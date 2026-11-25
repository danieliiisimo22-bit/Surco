#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod model;
mod process;
mod store;
#[cfg(windows)]
mod window;
mod worker;
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path as RoutePath, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use model::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tokio::sync::{Mutex, Notify};

pub struct App {
    root: PathBuf,
    data_dir: PathBuf,
    port: u16,
    profile: String,
    closing: std::sync::atomic::AtomicBool,
    shutdown: Notify,
    worker_error: Mutex<Option<String>>,
    token: String,
    config: Mutex<Config>,
    albums: Mutex<HashMap<String, Album>>,
    writes: Mutex<()>,
    active: Mutex<Option<(String, tokio::sync::watch::Sender<bool>)>>,
    started: Instant,
}
struct ApiError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":format!("{:#}",self.0)})),
        )
            .into_response()
    }
}
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;
macro_rules! ensure_api {
    ($condition:expr,$message:expr) => {
        if !$condition {
            return Err(anyhow::anyhow!($message).into());
        }
    };
}

async fn guard(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let port = app.port.to_string();
    if !matches!(host.rsplit_once(':'), Some(("127.0.0.1" | "localhost", p)) if p == port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = req.headers().get(header::ORIGIN)
        && origin.to_str().ok() != Some(&format!("http://{host}"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if req.method() != axum::http::Method::GET
        && req
            .headers()
            .get("x-surco-token")
            .and_then(|v| v.to_str().ok())
            != Some(&app.token)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Recarga Surco para continuar."})),
        )
            .into_response();
    }
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response.headers_mut().insert("content-security-policy","default-src 'self'; img-src 'self' https: data: blob:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'".parse().unwrap());
    response
}
async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(
        json!({"app":"Surco Rust","version":env!("CARGO_PKG_VERSION"),"uptime_ms":app.started.elapsed().as_millis(),"profile":app.profile}),
    )
}
async fn bootstrap(State(app): State<Arc<App>>) -> Json<Value> {
    let ready = app.root.join("bridge/catalog.py").is_file()
        && app.root.join("bridge/download.py").is_file()
        && process::python(&app.root).is_file()
        && process::runtime_tool(&app.root, "ffmpeg.exe").is_ok()
        && process::runtime_tool(&app.root, "node.exe").is_ok();
    let warnings =
        store::job_warnings(&app.data_dir).unwrap_or_else(|error| vec![error.to_string()]);
    Json(
        json!({"token": app.token, "config": *app.config.lock().await, "ready": ready, "warnings": warnings}),
    )
}
#[derive(Deserialize)]
struct Search {
    q: String,
}
async fn search(State(app): State<Arc<App>>, Query(q): Query<Search>) -> ApiResult<Value> {
    ensure_api!(
        !q.q.trim().is_empty() && q.q.len() <= 500,
        "Escribe un artista o álbum."
    );
    Ok(Json(
        process::bridge(&app.root, json!({"op":"search","q":q.q.trim()})).await?,
    ))
}
async fn lyrics_search(State(app): State<Arc<App>>, Query(q): Query<Search>) -> ApiResult<Value> {
    ensure_api!(
        !q.q.trim().is_empty() && q.q.len() <= 500,
        "Escribe una canción o un artista."
    );
    Ok(Json(
        process::bridge(&app.root, json!({"op":"search_lyrics","q":q.q.trim()})).await?,
    ))
}
async fn lyrics(State(app): State<Arc<App>>, RoutePath(id): RoutePath<String>) -> ApiResult<Value> {
    ensure_api!(
        id.len() == 11 && valid_id(&id),
        "Identificador de canción inválido."
    );
    Ok(Json(
        process::bridge(&app.root, json!({"op":"lyrics","id":id})).await?,
    ))
}
async fn album(State(app): State<Arc<App>>, RoutePath(id): RoutePath<String>) -> ApiResult<Album> {
    ensure_api!(valid_id(&id), "Identificador de álbum inválido.");
    let album: Album =
        serde_json::from_value(process::bridge(&app.root, json!({"op":"album","id":id})).await?)?;
    validate_album(&album)?;
    app.albums.lock().await.insert(id, album.clone());
    Ok(Json(album))
}
async fn artist(State(app): State<Arc<App>>, RoutePath(id): RoutePath<String>) -> ApiResult<Value> {
    ensure_api!(
        valid_id(&id) && id.starts_with("UC"),
        "Identificador de artista inválido."
    );
    Ok(Json(
        process::bridge(&app.root, json!({"op":"artist","id":id})).await?,
    ))
}
async fn jobs(State(app): State<Arc<App>>) -> ApiResult<Vec<Job>> {
    if let Some(error) = app.worker_error.lock().await.clone() {
        return Err(anyhow::anyhow!(error).into());
    }
    let dir = app.data_dir.clone();
    Ok(Json(
        tokio::task::spawn_blocking(move || -> Result<Vec<Job>> {
            let mut jobs = store::jobs(&dir)?;
            let is_active =
                |job: &Job| matches!(job.status.as_str(), "running" | "queued" | "paused");
            jobs.sort_by_key(|job| !is_active(job));
            let active = jobs.iter().take_while(|job| is_active(job)).count();
            Ok(jobs
                .into_iter()
                .take(active.max(100))
                .map(store::current)
                .collect())
        })
        .await??,
    ))
}
#[derive(Deserialize)]
struct Download {
    album_id: String,
    tracks: Vec<usize>,
}
async fn download(State(app): State<Arc<App>>, Json(input): Json<Download>) -> ApiResult<Value> {
    let mut indices = input.tracks;
    indices.sort_unstable();
    indices.dedup();
    let a = app
        .albums
        .lock()
        .await
        .get(&input.album_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Vuelve a abrir el álbum antes de descargar."))?;
    ensure_api!(
        !indices.is_empty() && indices.iter().all(|i| *i > 0 && *i <= a.tracks.len()),
        "Selecciona pistas válidas del álbum."
    );
    let config = app.config.lock().await.clone();
    let _write = app.writes.lock().await;
    for existing in store::jobs(&app.data_dir)? {
        if existing.album.id == a.id
            && existing.config.directory == config.directory
            && existing.config.format == config.format
        {
            let old: Vec<_> = existing.items.iter().map(|i| i.track.number).collect();
            if old != indices
                || existing.items.iter().any(|item| {
                    let Some(current) = item
                        .track
                        .number
                        .checked_sub(1)
                        .and_then(|index| a.tracks.get(index))
                    else {
                        return true;
                    };
                    item.track.video_id != current.video_id
                        || item.track.source_type != current.source_type
                        || item.track.unavailable != current.unavailable
                })
            {
                continue;
            }
            if ["running", "queued", "paused"].contains(&existing.status.as_str()) {
                return Ok(Json(json!({"id":existing.id,"duplicate":true})));
            }
            // The worker checks saved files off the async runtime before downloading.
            let mut j = existing;
            j.status = "queued".into();
            store::save(&store::job_path(&app.data_dir, &j.id), &j)?;
            return Ok(Json(json!({"id":j.id,"duplicate":true})));
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let id = format!("{stamp}-{}", uuid::Uuid::new_v4().simple());
    let items = indices
        .into_iter()
        .map(|i| Outcome {
            track: a.tracks[i - 1].clone(),
            status: "pending".into(),
            detail: String::new(),
            path: String::new(),
            lyrics: String::new(),
            hash: String::new(),
            backup: String::new(),
        })
        .collect();
    let job = Job {
        id: id.clone(),
        album: a,
        config,
        status: "queued".into(),
        items,
    };
    store::save(&store::job_path(&app.data_dir, &id), &job)?;
    Ok(Json(json!({"id":id,"duplicate":false})))
}
async fn control(
    State(app): State<Arc<App>>,
    RoutePath((id, action)): RoutePath<(String, String)>,
) -> ApiResult<Value> {
    let _write = app.writes.lock().await;
    let mut job = store::job(&app.data_dir, &id)?;
    match action.as_str() {
        "pause" | "cancel" => {
            ensure_api!(
                ["running", "queued", "paused"].contains(&job.status.as_str()),
                "Este trabajo ya terminó."
            );
            job.stop(if action == "pause" {
                "paused"
            } else {
                "cancelled"
            });
            store::save(&store::job_path(&app.data_dir, &id), &job)?;
            if let Some((active, tx)) = &*app.active.lock().await
                && active == &id
            {
                let _ = tx.send(true);
            }
        }
        "resume" => {
            ensure_api!(
                ["paused", "cancelled", "attention", "completed"].contains(&job.status.as_str()),
                "El trabajo ya está en curso."
            );
            job.status = "queued".into();
            job.config.browser = app.config.lock().await.browser.clone();
            store::save(&store::job_path(&app.data_dir, &id), &job)?;
        }
        _ => return Err(anyhow::anyhow!("Acción no reconocida.").into()),
    }
    Ok(Json(json!({"ok":true})))
}
async fn settings(State(app): State<Arc<App>>, Json(config): Json<Config>) -> ApiResult<Value> {
    ensure_api!(
        Path::new(&config.directory).is_absolute(),
        "La carpeta debe ser una ruta absoluta."
    );
    ensure_api!(
        ["mp3", "m4a"].contains(&config.format.as_str()),
        "Formato no válido."
    );
    ensure_api!(
        ["", "firefox", "chrome", "edge"].contains(&config.browser.as_str()),
        "Navegador no válido."
    );
    store::save(&app.data_dir.join("settings.json"), &config)?;
    *app.config.lock().await = config;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Folder {
    job_id: Option<String>,
}
async fn folder(State(app): State<Arc<App>>, Json(input): Json<Folder>) -> ApiResult<Value> {
    let path = if let Some(id) = input.job_id {
        let job = store::job(&app.data_dir, &id)?;
        job.items
            .iter()
            .find(|i| !i.path.is_empty() && Path::new(&i.path).is_file())
            .and_then(|i| Path::new(&i.path).parent())
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("Este trabajo todavía no tiene archivos guardados."))?
    } else {
        PathBuf::from(&app.config.lock().await.directory)
    };
    ensure_api!(
        path.is_dir(),
        "La carpeta todavía no existe; se creará al guardar la primera pista."
    );
    process::command("explorer.exe")
        .arg(&path)
        .kill_on_drop(false)
        .spawn()?;
    Ok(Json(json!({"path":path})))
}
async fn report(
    State(app): State<Arc<App>>,
    RoutePath(id): RoutePath<String>,
) -> std::result::Result<Response, ApiError> {
    let job = store::current(store::job(&app.data_dir, &id)?);
    let e = |v: &str| {
        v.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let mut rows = String::new();
    for i in &job.items {
        let status = match i.status.as_str() {
            "saved" => "Guardada",
            "missing" => "Archivo no localizado",
            "error" => "No guardada",
            _ => "Pendiente",
        };
        rows.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",i.track.number,e(&i.track.title),status,e(&i.detail),e(&i.lyrics),e(&i.path),e(&i.backup)));
    }
    let done = job.items.iter().filter(|i| i.status == "saved").count();
    let body = format!(
        "<!doctype html><html lang=es><meta charset=utf-8><title>Resultados Surco</title><style>body{{font:15px system-ui;margin:32px;color:#283725}}table{{border-collapse:collapse;width:100%}}td,th{{border:1px solid #ccc;padding:10px;text-align:left;overflow-wrap:anywhere;max-width:300px}}</style><h1>{}</h1><p>{} · {} · {} de {} pistas guardadas</p><p>Estado al exportar. Consulta Descargas para ver cambios posteriores.</p><table><tr><th>Pista</th><th>Canción</th><th>Estado</th><th>Resultado</th><th>Letra</th><th>Archivo</th><th>Respaldo</th></tr>{}</table></html>",
        e(&job.album.title),
        e(&job.album.artist),
        e(&job.album.year),
        done,
        job.items.len(),
        rows
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=Surco-resultados.html",
            ),
        ],
        body,
    )
        .into_response())
}
async fn shutdown(State(app): State<Arc<App>>) -> Json<Value> {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        app.shutdown.notify_one();
    });
    Json(json!({"ok":true}))
}
async fn stop(app: &App) -> Result<()> {
    let _write = app.writes.lock().await;
    app.closing
        .store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some((id, tx)) = &*app.active.lock().await {
        let saved = store::job(&app.data_dir, id).and_then(|mut job| {
            if job.status == "running" {
                job.stop("paused");
                store::save(&store::job_path(&app.data_dir, id), &job)?;
            }
            Ok(())
        });
        let _ = tx.send(true);
        saved?;
    }
    Ok(())
}
fn router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/wallpaper.webp",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/webp")],
                    &include_bytes!("../web/wallpaper.webp")[..],
                )
            }),
        )
        .route(
            "/surco.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../web/surco.svg"),
                )
            }),
        )
        .route(
            "/material.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/material.js"),
                )
            }),
        )
        .route(
            "/glasswork/lens.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/glasswork/lens.js"),
                )
            }),
        )
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/search", get(search))
        .route("/api/lyrics/search", get(lyrics_search))
        .route("/api/lyrics/{id}", get(lyrics))
        .route("/api/artists/{id}", get(artist))
        .route("/api/albums/{id}", get(album))
        .route("/api/jobs", get(jobs))
        .route("/api/download", post(download))
        .route("/api/jobs/{id}/{action}", post(control))
        .route("/api/report/{id}", get(report))
        .route("/api/settings", post(settings))
        .route("/api/folder", post(folder))
        .route("/api/shutdown", post(shutdown))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}
const OTHER_PROFILE: &str =
    "Surco ya está abierto en otra sesión de Windows. Ciérralo desde allí para usarlo aquí.";

fn check_running_instance(response: &[u8], profile: &str) -> Result<()> {
    let response = std::str::from_utf8(response)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .context("Respuesta inválida en el puerto de Surco.")?;
    anyhow::ensure!(
        headers.starts_with("HTTP/1.1 200 "),
        "Otro programa ocupa el puerto de Surco (8737)."
    );
    let health: Value =
        serde_json::from_str(body).context("Otro programa ocupa el puerto de Surco (8737).")?;
    anyhow::ensure!(
        health["app"] == "Surco Rust",
        "Otro programa ocupa el puerto de Surco (8737)."
    );
    anyhow::ensure!(health["profile"] == profile, "{OTHER_PROFILE}");
    Ok(())
}

async fn run() -> Result<()> {
    let start = Instant::now();
    let headless = std::env::args().any(|arg| arg == "--headless");
    let port = if std::env::args().any(|arg| arg == "--ephemeral-port") {
        0
    } else {
        8737
    };
    let root = std::env::current_exe()?.parent().unwrap().to_path_buf();
    #[cfg(debug_assertions)]
    let root = if root.join("bridge/catalog.py").is_file() {
        root
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    };
    let data_dir = store::data(&root);
    std::fs::create_dir_all(&data_dir)?;
    let profile = store::profile_id(&data_dir)?;
    let listener = match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
    {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut stream = tokio::net::TcpStream::connect("127.0.0.1:8737").await?;
            stream.write_all(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:8737\r\nConnection: close\r\n\r\n").await?;
            let mut response = Vec::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                stream.read_to_end(&mut response),
            )
            .await??;
            check_running_instance(&response, &profile)?;
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    let default = Config {
        directory: std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone())
            .join("Music")
            .join("Surco")
            .to_string_lossy()
            .into(),
        format: "mp3".into(),
        browser: String::new(),
    };
    let settings_path = data_dir.join("settings.json");
    let config = if settings_path.is_file() {
        store::read(&settings_path)?
    } else {
        store::save(&settings_path, &default)?;
        default
    };
    for mut j in store::jobs(&data_dir)? {
        if j.status == "running" {
            j.stop("paused");
            store::save(&store::job_path(&data_dir, &j.id), &j)?;
        }
    }
    let app = Arc::new(App {
        root,
        data_dir,
        port: listener.local_addr()?.port(),
        profile,
        closing: false.into(),
        shutdown: Notify::new(),
        worker_error: Mutex::new(None),
        config: Mutex::new(config),
        token: uuid::Uuid::new_v4().to_string(),
        albums: Mutex::new(HashMap::new()),
        writes: Mutex::new(()),
        active: Mutex::new(None),
        started: start,
    });
    store::save(
        &app.data_dir.join("startup.json"),
        &json!({"ready_ms":start.elapsed().as_millis(),"version":env!("CARGO_PKG_VERSION"),"port":app.port}),
    )?;
    let worker = tokio::spawn(worker::dispatch(app.clone()));
    let server = async { axum::serve(listener, router(app.clone())).await };
    let desktop = async {
        if headless {
            return std::future::pending().await;
        }
        #[cfg(windows)]
        return window::run(&app.data_dir, app.port).await;
        #[cfg(not(windows))]
        anyhow::bail!("La ventana de Surco requiere Windows.")
    };
    let result = tokio::select! {
        result = server => result.map_err(Into::into),
        result = desktop => result,
        _ = app.shutdown.notified() => Ok(()),
    };
    let stopped = stop(&app).await;
    worker.await?;
    result?;
    stopped
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e:#}");
        if let Ok(exe) = std::env::current_exe()
            && let Some(root) = exe.parent()
        {
            let dir = store::data(root);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("surco-error.log"), format!("{e:#}"));
        }
        #[cfg(windows)]
        if !std::env::args().any(|arg| arg == "--headless") {
            process::show_error(&format!("{e:#}"));
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    fn test_app(root: &Path) -> Arc<App> {
        Arc::new(App {
            root: root.into(),
            data_dir: root.into(),
            port: 8737,
            profile: store::profile_id(root).unwrap(),
            closing: false.into(),
            shutdown: Notify::new(),
            worker_error: Mutex::new(None),
            token: "test-token".into(),
            config: Mutex::new(Config {
                directory: root.join("music").to_string_lossy().into(),
                format: "mp3".into(),
                browser: String::new(),
            }),
            albums: Mutex::new(HashMap::new()),
            writes: Mutex::new(()),
            active: Mutex::new(None),
            started: Instant::now(),
        })
    }
    fn req(method: &str, path: &str, token: &str, body: Value) -> Request {
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:8737")
            .header("x-surco-token", token)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }
    #[tokio::test]
    async fn write_requires_local_token() {
        let d = tempfile::tempdir().unwrap();
        let a = test_app(d.path());
        let r = router(a)
            .oneshot(req("POST", "/api/download", "wrong", json!({})))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }
    #[tokio::test]
    async fn album_enqueues_once_and_keeps_selected_publication() {
        let d = tempfile::tempdir().unwrap();
        let a = test_app(d.path());
        let mut album: Album =
            serde_json::from_str(include_str!("../tests/fixtures/beatles-manifest.json")).unwrap();
        album.tracks[0].source_album = "Otra publicación".into();
        a.albums
            .lock()
            .await
            .insert(album.id.clone(), album.clone());
        let input = json!({"album_id":album.id,"tracks":[1]});
        let r = router(a.clone())
            .oneshot(req("POST", "/api/download", "test-token", input.clone()))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let r = router(a.clone())
            .oneshot(req("POST", "/api/download", "test-token", input))
            .await
            .unwrap();
        let v: Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["duplicate"], true);
        let jobs = store::jobs(d.path()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].album.title, album.title);
        assert_eq!(jobs[0].items[0].track.source_album, "Otra publicación");

        // A fresh catalog source must not requeue the superseded source.
        album.tracks[0].video_id = "abcdefghijk".into();
        let id = album.id.clone();
        a.albums.lock().await.insert(id.clone(), album);
        let r = router(a)
            .oneshot(req(
                "POST",
                "/api/download",
                "test-token",
                json!({"album_id":id,"tracks":[1]}),
            ))
            .await
            .unwrap();
        let v: Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["duplicate"], false);
        assert_eq!(store::jobs(d.path()).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn download_enqueues_all_selected_tracks_in_album_order() {
        let d = tempfile::tempdir().unwrap();
        let app = test_app(d.path());
        let album: Album =
            serde_json::from_str(include_str!("../tests/fixtures/beatles-manifest.json")).unwrap();
        let id = album.id.clone();
        app.albums.lock().await.insert(id.clone(), album);

        let send = |tracks: &[usize]| {
            router(app.clone()).oneshot(req(
                "POST",
                "/api/download",
                "test-token",
                json!({"album_id":id,"tracks":tracks}),
            ))
        };
        let rejected = send(&[0, 1, 2, 3]).await.unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        assert!(store::jobs(d.path()).unwrap().is_empty());

        let accepted = send(&[3, 1, 1, 2]).await.unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
        let jobs = store::jobs(d.path()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, "queued");
        let numbers: Vec<_> = jobs[0].items.iter().map(|item| item.track.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert!(jobs[0].items.iter().all(|item| item.status == "pending"));
    }
    #[tokio::test]
    async fn unknown_album_cannot_inject_a_source() {
        let d = tempfile::tempdir().unwrap();
        let a = test_app(d.path());
        let r = router(a)
            .oneshot(req(
                "POST",
                "/api/download",
                "test-token",
                json!({"album_id":"MPRE_unknown","tracks":[1],"video_id":"abcdefghijk"}),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
    #[tokio::test]
    async fn health_distinguishes_data_profiles_without_returning_paths() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut profiles = Vec::new();
        for dir in [&first, &second] {
            let response = router(test_app(dir.path()))
                .oneshot(req("GET", "/api/health", "", json!(null)))
                .await
                .unwrap();
            let value: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            let profile = value["profile"].as_str().unwrap();
            assert_eq!(profile.len(), 64);
            assert_eq!(profile, store::profile_id(dir.path()).unwrap());
            profiles.push(profile.to_owned());
        }
        assert_ne!(profiles[0], profiles[1]);
        let response = format!(
            "HTTP/1.1 200 OK\r\n\r\n{}",
            json!({"app":"Surco Rust","profile":profiles[0]})
        );
        assert!(check_running_instance(response.as_bytes(), &profiles[0]).is_ok());
        assert_eq!(
            check_running_instance(response.as_bytes(), &profiles[1])
                .unwrap_err()
                .to_string(),
            OTHER_PROFILE
        );
    }
    #[tokio::test]
    async fn controls_keep_saved_tracks_on_resume() {
        let d = tempfile::tempdir().unwrap();
        let a = test_app(d.path());
        let album: Album =
            serde_json::from_str(include_str!("../tests/fixtures/beatles-manifest.json")).unwrap();
        a.albums
            .lock()
            .await
            .insert(album.id.clone(), album.clone());
        let _ = download(
            State(a.clone()),
            Json(Download {
                album_id: album.id.clone(),
                tracks: vec![1, 2],
            }),
        )
        .await
        .unwrap_or_else(|_| panic!("enqueue"));
        let mut j = store::jobs(d.path()).unwrap().remove(0);
        j.status = "running".into();
        j.items[0].status = "downloading".into();
        j.items[0].detail = "Descargando".into();
        j.items[1].status = "saved".into();
        store::save(&store::job_path(d.path(), &j.id), &j).unwrap();
        let _ = control(State(a.clone()), RoutePath((j.id.clone(), "pause".into())))
            .await
            .unwrap_or_else(|_| panic!("pause"));
        let paused = store::job(d.path(), &j.id).unwrap();
        assert_eq!(paused.status, "paused");
        assert_eq!(paused.items[0].status, "pending");
        assert!(paused.items[0].detail.is_empty());
        assert_eq!(paused.items[1].status, "saved");
        let _ = control(State(a), RoutePath((j.id.clone(), "resume".into())))
            .await
            .unwrap_or_else(|_| panic!("resume"));
        assert_eq!(store::job(d.path(), &j.id).unwrap().status, "queued");
    }

    #[tokio::test]
    async fn shutdown_pauses_the_active_album_and_keeps_the_queue() {
        let d = tempfile::tempdir().unwrap();
        let app = test_app(d.path());
        let album: Album =
            serde_json::from_str(include_str!("../tests/fixtures/beatles-manifest.json")).unwrap();
        app.albums
            .lock()
            .await
            .insert(album.id.clone(), album.clone());
        for tracks in [vec![1, 2, 3], vec![4]] {
            let _ = download(
                State(app.clone()),
                Json(Download {
                    album_id: album.id.clone(),
                    tracks,
                }),
            )
            .await
            .unwrap_or_else(|_| panic!("enqueue"));
        }
        let jobs = store::jobs(d.path()).unwrap();
        let mut active = jobs
            .iter()
            .find(|job| job.items.len() == 3)
            .unwrap()
            .clone();
        active.status = "running".into();
        active.items[0].status = "saved".into();
        active.items[1].status = "downloading".into();
        store::save(&store::job_path(d.path(), &active.id), &active).unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        *app.active.lock().await = Some((active.id.clone(), tx));

        stop(&app).await.unwrap();

        assert!(*rx.borrow());
        let paused = store::job(d.path(), &active.id).unwrap();
        assert_eq!(paused.status, "paused");
        assert_eq!(paused.items[0].status, "saved");
        assert_eq!(paused.items[1].status, "pending");
        assert!(
            store::jobs(d.path())
                .unwrap()
                .iter()
                .any(|job| job.status == "queued")
        );
    }
}

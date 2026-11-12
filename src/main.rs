#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod model;
mod process;
mod store;
mod worker;
use anyhow::Result;
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
use tokio::sync::Mutex;

pub struct App {
    root: PathBuf,
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
    if !["127.0.0.1:8737", "localhost:8737"].contains(&host) {
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
        json!({"app":"Surco Rust","version":"1.0.0","uptime_ms":app.started.elapsed().as_millis()}),
    )
}
async fn bootstrap(State(app): State<Arc<App>>) -> Json<Value> {
    let ready = process::python(&app.root).is_file()
        && process::runtime_tool(&app.root, "ffmpeg.exe").is_ok()
        && process::runtime_tool(&app.root, "node.exe").is_ok();
    Json(json!({"token": app.token, "config": *app.config.lock().await, "ready": ready}))
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
    Ok(Json(
        store::jobs(&app.root)?
            .into_iter()
            .filter(|j| !j.id.starts_with("repair-"))
            .map(store::current)
            .collect(),
    ))
}
#[derive(Deserialize)]
struct Download {
    album_id: String,
    tracks: Vec<usize>,
}
async fn download(State(app): State<Arc<App>>, Json(input): Json<Download>) -> ApiResult<Value> {
    let a = app
        .albums
        .lock()
        .await
        .get(&input.album_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Vuelve a abrir el álbum antes de descargar."))?;
    let mut indices = input.tracks;
    indices.sort();
    indices.dedup();
    ensure_api!(
        !indices.is_empty() && indices.iter().all(|i| *i > 0 && *i <= a.tracks.len()),
        "Selecciona pistas válidas del álbum."
    );
    let config = app.config.lock().await.clone();
    let _write = app.writes.lock().await;
    for existing in store::jobs(&app.root)? {
        if !existing.id.starts_with("repair-")
            && existing.album.id == a.id
            && existing.config.directory == config.directory
            && existing.config.format == config.format
        {
            let old: Vec<_> = existing.items.iter().map(|i| i.track.number).collect();
            if old != indices
                || existing.items.iter().any(|item| {
                    let current = &a.tracks[item.track.number - 1];
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
            if existing.items.iter().all(|i| {
                i.status == "saved"
                    && !i.hash.is_empty()
                    && store::hash(Path::new(&i.path)).is_ok_and(|hash| hash == i.hash)
            }) {
                return Ok(Json(json!({"id":existing.id,"duplicate":true})));
            }
            let mut j = existing;
            j.status = "queued".into();
            store::save(&store::job_path(&app.root, &j.id), &j)?;
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
    store::save(&store::job_path(&app.root, &id), &job)?;
    Ok(Json(json!({"id":id,"duplicate":false})))
}
async fn control(
    State(app): State<Arc<App>>,
    RoutePath((id, action)): RoutePath<(String, String)>,
) -> ApiResult<Value> {
    let _write = app.writes.lock().await;
    ensure_api!(
        !id.starts_with("repair-"),
        "Este trabajo pertenece a una versión anterior."
    );
    let mut job = store::job(&app.root, &id)?;
    match action.as_str() {
        "pause" | "cancel" => {
            ensure_api!(
                ["running", "queued", "paused"].contains(&job.status.as_str()),
                "Este trabajo ya terminó."
            );
            job.status = if action == "pause" {
                "paused"
            } else {
                "cancelled"
            }
            .into();
            store::save(&store::job_path(&app.root, &id), &job)?;
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
            store::save(&store::job_path(&app.root, &id), &job)?;
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
    store::save(&store::data(&app.root).join("settings.json"), &config)?;
    *app.config.lock().await = config;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Folder {
    job_id: Option<String>,
}
async fn folder(State(app): State<Arc<App>>, Json(input): Json<Folder>) -> ApiResult<Value> {
    let path = if let Some(id) = input.job_id {
        let job = store::job(&app.root, &id)?;
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
    let job = store::current(store::job(&app.root, &id)?);
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
    let _write = app.writes.lock().await;
    if let Some((id, tx)) = &*app.active.lock().await {
        if let Ok(mut j) = store::job(&app.root, id) {
            j.status = "paused".into();
            let _ = store::save(&store::job_path(&app.root, id), &j);
        }
        let _ = tx.send(true);
    }
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        std::process::exit(0)
    });
    Json(json!({"ok":true}))
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
fn open_window() {
    let paths = [
        std::env::var("ProgramFiles(x86)").unwrap_or_default(),
        std::env::var("ProgramFiles").unwrap_or_default(),
    ];
    for base in paths {
        let edge = Path::new(&base).join("Microsoft/Edge/Application/msedge.exe");
        if edge.is_file() {
            let mut c = std::process::Command::new(edge);
            c.args([
                "--app=http://127.0.0.1:8737",
                "--new-window",
                "--window-size=1280,900",
            ]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x08000000);
            }
            let _ = c.spawn();
            return;
        }
    }
    let _ = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", "http://127.0.0.1:8737"])
        .spawn();
}
async fn run() -> Result<()> {
    let start = Instant::now();
    let root = std::env::current_exe()?.parent().unwrap().to_path_buf();
    let root = if root.join("bridge/catalog.py").is_file() {
        root
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    };
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:8737").await {
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
            anyhow::ensure!(
                String::from_utf8_lossy(&response).contains("Surco Rust"),
                "Otro programa ocupa el puerto de Surco (8737)."
            );
            if !std::env::args().any(|a| a == "--headless") {
                open_window();
            }
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
    let settings_path = store::data(&root).join("settings.json");
    let config = if settings_path.is_file() {
        store::read(&settings_path)?
    } else {
        store::save(&settings_path, &default)?;
        default
    };
    for mut j in store::jobs(&root)? {
        if j.status == "running" && !j.id.starts_with("repair-") {
            j.status = "paused".into();
            store::save(&store::job_path(&root, &j.id), &j)?;
        }
    }
    let app = Arc::new(App {
        root,
        config: Mutex::new(config),
        token: uuid::Uuid::new_v4().to_string(),
        albums: Mutex::new(HashMap::new()),
        writes: Mutex::new(()),
        active: Mutex::new(None),
        started: start,
    });
    tokio::spawn(worker::dispatch(app.clone()));
    store::save(
        &store::data(&app.root).join("startup.json"),
        &json!({"ready_ms":start.elapsed().as_millis(),"version":"1.0.0"}),
    )?;
    if !std::env::args().any(|a| a == "--headless") {
        open_window();
    }
    axum::serve(listener, router(app)).await?;
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e:#}");
        let _ = std::fs::write("surco-error.log", format!("{e:#}"));
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
                tracks: vec![1],
            }),
        )
        .await
        .unwrap_or_else(|_| panic!("enqueue"));
        let j = store::jobs(d.path()).unwrap().remove(0);
        let _ = control(State(a.clone()), RoutePath((j.id.clone(), "pause".into())))
            .await
            .unwrap_or_else(|_| panic!("pause"));
        assert_eq!(store::job(d.path(), &j.id).unwrap().status, "paused");
        let _ = control(State(a), RoutePath((j.id.clone(), "resume".into())))
            .await
            .unwrap_or_else(|_| panic!("resume"));
        assert_eq!(store::job(d.path(), &j.id).unwrap().status, "queued");
    }
}

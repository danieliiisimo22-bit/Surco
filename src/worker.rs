use crate::{App, model::*, process, store};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

async fn update(app: &App, id: &str, index: usize, status: &str, detail: &str) -> Result<()> {
    let _lock = app.writes.lock().await;
    let mut job = store::job(&app.root, id)?;
    if job.status != "running" {
        bail!("El trabajo está detenido.");
    }
    job.items[index].status = status.into();
    job.items[index].detail = detail.into();
    store::save(&store::job_path(&app.root, id), &job)
}

fn candidates(app: &App, job: &Job, t: &Track) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for j in store::jobs(&app.root)? {
        for i in j.items {
            if i.status == "saved"
                && i.track.video_id == t.video_id
                && j.config.format == job.config.format
                && Path::new(&i.path).is_file()
            {
                result.push(PathBuf::from(i.path));
            }
        }
    }
    let inventory = store::data(&app.root).join("existing-library.json");
    if inventory.is_file() {
        let rows: Vec<Value> = store::read(&inventory)?;
        for row in rows {
            if row["video_id"] == t.video_id && row["format"] == job.config.format {
                let p = PathBuf::from(row["path"].as_str().unwrap_or(""));
                if p.is_file() {
                    result.push(p);
                }
            }
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

async fn saved_valid(app: &App, item: &Outcome) -> bool {
    if item.status != "saved" || item.hash.is_empty() {
        return false;
    }
    let p = PathBuf::from(&item.path);
    let expected = item.hash.clone();
    let result = tokio::task::spawn_blocking(move || store::hash(&p)).await;
    matches!(result,Ok(Ok(hash)) if hash==expected) && app.root.is_dir()
}

async fn track(app: &App, job: &Job, index: usize, cover: &Path) -> Result<Outcome> {
    let t = &job.items[index].track;
    anyhow::ensure!(!t.unavailable, "{}", t.reason);
    update(
        app,
        &job.id,
        index,
        "checking",
        "Comprobando la pista de esta edición…",
    )
    .await?;
    let source = process::bridge(&app.root, json!({"op":"source","id":t.video_id})).await?;
    validate_source(t, &source)?;
    let expected_duration = if t.duration > 0. {
        t.duration
    } else {
        source["duration"].as_f64().unwrap_or(0.)
    };
    let target = store::destination(&job.config, &job.album, t);
    let parent = target.parent().context("Carpeta inválida")?;
    fs::create_dir_all(parent)?;
    // Persist partial work so pauses and tag/write failures don't download again.
    // Publication stays on the destination volume and never overwrites a file.
    let stage = parent
        .join(".surco-work")
        .join(&job.id)
        .join(format!("{index}-{}", t.video_id));
    fs::create_dir_all(&stage)?;
    let prepared = stage.join(format!("prepared.{}", job.config.format));
    let checkpoint = stage.join("progress.json");
    let mut progress: Value = if checkpoint.is_file() {
        store::read(&checkpoint)?
    } else {
        json!({})
    };
    let mut reused = false;
    let mut replace_hash = None;
    if progress["converted"] != true || !prepared.is_file() {
        for candidate in candidates(app, job, t)? {
            let proof = process::bridge(&app.root, json!({"op":"inspect","path":candidate})).await;
            if let Ok(proof) = proof
                && proof["source_id"] == t.video_id
                && duration_matches(expected_duration, proof["duration"].as_f64().unwrap_or(0.))
            {
                update(
                    app,
                    &job.id,
                    index,
                    "tagging",
                    "Reutilizando el audio local para este álbum; sin descargar ni convertir.",
                )
                .await?;
                fs::copy(&candidate, &prepared)?;
                reused = true;
                progress["converted"] = true.into();
                progress["reused"] = true.into();
                store::save(&checkpoint, &progress)?;
                break;
            }
        }
    } else {
        reused = progress["reused"] == true;
    }
    if target.exists() {
        let proof = process::bridge(&app.root, json!({"op":"inspect","path":target})).await?;
        let same_audio = proof["source_id"] == t.video_id
            && duration_matches(expected_duration, proof["duration"].as_f64().unwrap_or(0.));
        // A completed write interrupted before the final job save can be
        // recovered only from the durable receipt made before publication.
        let receipt = store::data(&app.root)
            .join("receipts")
            .join(format!("{}-{index}.json", job.id));
        if same_audio && receipt.is_file() {
            let r: Outcome = store::read(&receipt)?;
            if r.track.video_id == t.video_id
                && Path::new(&r.path) == target
                && store::hash(&target)? == r.hash
            {
                return Ok(r);
            }
        }
        if same_audio
            && key(proof["title"].as_str().unwrap_or("")) == key(&t.title)
            && key(proof["album"].as_str().unwrap_or("")) == key(&job.album.title)
            && proof["number"].as_u64() == Some(t.number as u64)
            && (proof["lyrics"] == true || source["lyrics"].as_str().unwrap_or("").is_empty())
        {
            return Ok(Outcome {
                track: t.clone(),
                status: "saved".into(),
                path: target.to_string_lossy().into(),
                hash: store::hash(&target)?,
                detail:
                    "La pista exacta ya estaba guardada en este álbum; no se volvió a descargar."
                        .into(),
                lyrics: if proof["lyrics"] == true {
                    "Incorporada"
                } else {
                    "Sin letra incorporada en el archivo existente"
                }
                .into(),
                backup: String::new(),
            });
        }
        if proof["source_id"] == t.video_id {
            replace_hash = Some(store::hash(&target)?);
            if same_audio {
                fs::copy(&target, &prepared)?;
                reused = true;
                progress["converted"] = true.into();
                progress["reused"] = true.into();
                store::save(&checkpoint, &progress)?;
            }
        } else {
            bail!(
                "Ya existe un archivo en {}. No se sobrescribió: su pertenencia a esta edición no está registrada en este trabajo.",
                target.display()
            );
        }
    }
    if !reused && (progress["converted"] != true || !prepared.is_file()) {
        update(
            app,
            &job.id,
            index,
            "downloading",
            "Descargando el audio de la lista del álbum…",
        )
        .await?;
        let mut cmd = process::command(process::python(&app.root));
        cmd.arg(app.root.join("bridge/download.py"))
            .args([
                "--ignore-config",
                "--no-playlist",
                "--no-progress",
                "--no-warnings",
                "--no-overwrites",
                "--retries",
                "2",
                "--socket-timeout",
                "25",
                "--fixup",
                "never",
            ])
            .arg("--js-runtimes")
            .arg(format!(
                "node:{}",
                process::runtime_tool(&app.root, "node.exe")?.display()
            ))
            .arg("--format")
            .arg(if job.config.format == "m4a" {
                "bestaudio[ext=m4a]"
            } else {
                "bestaudio"
            })
            .arg("--output")
            .arg(stage.join("source.%(ext)s"))
            .arg("--print")
            .arg("after_move:filepath");
        if !job.config.browser.is_empty() {
            cmd.arg("--cookies-from-browser").arg(&job.config.browser);
        }
        cmd.arg(format!("https://music.youtube.com/watch?v={}", t.video_id));
        let output = process::run(cmd, 600).await?;
        let raw = std::str::from_utf8(&output.stdout).context("El motor devolvió una ruta con codificación inválida; el archivo descargado se conserva para reintentar.")?;
        let input = raw
            .lines()
            .last()
            .context("El motor no indicó el archivo descargado.")?;
        let input = PathBuf::from(input.trim());
        anyhow::ensure!(
            input.is_file() && input.canonicalize()?.starts_with(stage.canonicalize()?),
            "La descarga no terminó en la carpeta temporal prevista."
        );
        update(
            app,
            &job.id,
            index,
            "converting",
            if job.config.format == "mp3" {
                "Preparando MP3 320…"
            } else {
                "Preparando M4A sin recodificar…"
            },
        )
        .await?;
        let mut ff = process::command(process::runtime_tool(&app.root, "ffmpeg.exe")?);
        ff.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
            .arg(input)
            .args(["-map", "0:a:0", "-vn", "-map_metadata", "-1"]);
        if job.config.format == "mp3" {
            ff.args(["-c:a", "libmp3lame", "-b:a", "320k"]);
        } else {
            ff.args(["-c:a", "copy"]);
        }
        ff.arg(&prepared);
        process::run(ff, 300).await?;
        progress["converted"] = true.into();
        store::save(&checkpoint, &progress)?;
    }
    update(
        app,
        &job.id,
        index,
        "tagging",
        "Incorporando los datos del álbum, portada y letra disponible…",
    )
    .await?;
    let tagged=process::bridge(&app.root,json!({"op":"tag","path":prepared,"cover":cover,"album":job.album,"track":t,"lyrics":source["lyrics"],"lyrics_status":source["lyrics_status"]})).await?;
    let duration = tagged["duration"].as_f64().unwrap_or(0.);
    anyhow::ensure!(
        duration_matches(expected_duration, duration),
        "La duración del archivo no coincide con la pista del álbum. No se publicó el archivo."
    );
    let mut ff = process::command(process::runtime_tool(&app.root, "ffmpeg.exe")?);
    ff.args(["-v", "error", "-xerror", "-nostdin", "-i"])
        .arg(&prepared)
        .args(["-map", "0:a:0", "-f", "null", "-"]);
    process::run(ff, 180)
        .await
        .context("No se pudo verificar la integridad del audio")?;
    let hash = store::hash(&prepared)?;
    let mut out = Outcome {
        track: t.clone(),
        status: "saved".into(),
        path: target.to_string_lossy().into(),
        lyrics: tagged["lyrics"].as_str().unwrap_or("Sin letra").into(),
        hash,
        backup: String::new(),
        detail: if reused {
            "Audio local reutilizado; etiquetado para este álbum."
        } else {
            "Audio del álbum descargado y comprobado."
        }
        .into(),
    };
    if let Some(expected) = &replace_hash {
        anyhow::ensure!(
            store::hash(&target)? == *expected,
            "El archivo cambió durante la preparación. No se sobrescribió."
        );
        let folder = parent.join(".surco-backups");
        fs::create_dir_all(&folder)?;
        let backup = folder.join(format!(
            "{}-{index}-{}.bak",
            job.id,
            uuid::Uuid::new_v4().simple()
        ));
        fs::copy(&target, &backup)?;
        anyhow::ensure!(
            store::hash(&backup)? == *expected && store::hash(&target)? == *expected,
            "El respaldo no se pudo comprobar. No se sobrescribió el archivo."
        );
        out.backup = backup.to_string_lossy().into();
    }
    // Save evidence before publication. A restart can recover the exact result.
    store::save(
        &store::data(&app.root)
            .join("receipts")
            .join(format!("{}-{index}.json", job.id)),
        &out,
    )?;
    fs::OpenOptions::new()
        .write(true)
        .open(&prepared)?
        .sync_all()
        .context("No se pudo confirmar la escritura del audio")?;
    let temp = tempfile::TempPath::try_from_path(&prepared)?;
    let published = if replace_hash.is_some() {
        temp.persist(&target)
    } else {
        temp.persist_noclobber(&target)
    };
    published
        .map_err(|e| {
            let _ = e.path.keep();
            e.error
        })
        .context("No se pudo publicar el archivo. El audio anterior se conserva.")?;
    Ok(out)
}

async fn run_job(app: Arc<App>, id: String) -> Result<()> {
    let job = store::job(&app.root, &id)?;
    let fresh: Album = serde_json::from_value(
        process::bridge(&app.root, json!({"op":"album","id":job.album.id})).await?,
    )?;
    validate_album(&fresh)?;
    anyhow::ensure!(
        fresh.playlist_id == job.album.playlist_id && key(&fresh.title) == key(&job.album.title),
        "La publicación cambió desde la selección. Vuelve a abrir el álbum."
    );
    for item in &job.items {
        let now = fresh
            .tracks
            .get(item.track.number - 1)
            .context("Cambió el orden del álbum")?;
        anyhow::ensure!(
            now.video_id == item.track.video_id && key(&now.title) == key(&item.track.title),
            "Cambió una pista de la publicación. No se elegirá otra versión automáticamente."
        );
    }
    let covers = store::data(&app.root).join("covers");
    fs::create_dir_all(&covers)?;
    let cover = covers.join(format!("{}.jpg", job.album.id));
    if !cover.is_file() && !job.album.image.is_empty() {
        let _ = process::bridge(
            &app.root,
            json!({"op":"cover","url":job.album.image,"path":cover}),
        )
        .await;
    }
    for index in 0..job.items.len() {
        let latest = store::job(&app.root, &id)?;
        if saved_valid(&app, &latest.items[index]).await {
            continue;
        }
        let result = track(&app, &latest, index, &cover).await;
        let _lock = app.writes.lock().await;
        let mut latest = store::job(&app.root, &id)?;
        if latest.status != "running" {
            return Ok(());
        }
        match result {
            Ok(out) => latest.items[index] = out,
            Err(e) => {
                latest.items[index].status = "error".into();
                latest.items[index].detail = format!("{e:#}");
            }
        }
        store::save(&store::job_path(&app.root, &id), &latest)?;
        if latest.items[index].status == "saved" {
            let t = &latest.items[index].track;
            let target = store::destination(&latest.config, &latest.album, t);
            if let Some(parent) = target.parent() {
                let own = parent
                    .join(".surco-work")
                    .join(&id)
                    .join(format!("{index}-{}", t.video_id));
                // Only this track's private work directory; never an album.
                let _ = fs::remove_dir_all(&own);
                if let Some(job_dir) = own.parent() {
                    let _ = fs::remove_dir(job_dir);
                    if let Some(work) = job_dir.parent() {
                        let _ = fs::remove_dir(work);
                    }
                }
            }
        }
    }
    let _lock = app.writes.lock().await;
    let mut latest = store::job(&app.root, &id)?;
    latest.status = if latest.items.iter().all(|x| x.status == "saved") {
        "completed"
    } else {
        "attention"
    }
    .into();
    store::save(&store::job_path(&app.root, &id), &latest)?;
    Ok(())
}

pub async fn dispatch(app: Arc<App>) {
    loop {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let selected = {
            let _lock = app.writes.lock().await;
            let jobs = store::jobs(&app.root).unwrap_or_default();
            let mut next = jobs
                .into_iter()
                .rev()
                .find(|j| j.status == "queued" && !j.id.starts_with("repair-"));
            if let Some(j) = &mut next {
                j.status = "running".into();
                if store::save(&store::job_path(&app.root, &j.id), j).is_err() {
                    next = None;
                }
            }
            next
        };
        if let Some(job) = selected {
            let (tx, mut rx) = tokio::sync::watch::channel(false);
            *app.active.lock().await = Some((job.id.clone(), tx));
            let result =
                tokio::select! {r=run_job(app.clone(),job.id.clone())=>r,_=rx.changed()=>Ok(())};
            if let Err(e) = result {
                let _lock = app.writes.lock().await;
                if let Ok(mut j) = store::job(&app.root, &job.id)
                    && j.status == "running"
                {
                    j.status = "attention".into();
                    for i in &mut j.items {
                        if i.status != "saved" {
                            i.status = "error".into();
                            i.detail = format!("{e:#}");
                        }
                    }
                    let _ = store::save(&store::job_path(&app.root, &j.id), &j);
                }
            }
            *app.active.lock().await = None;
        }
    }
}

use crate::model::*;
use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path).with_context(
        || format!("No se pudo leer {}", path.display()),
    )?)?)
}
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("Ruta de datos inválida")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn data_path(
    root: &Path,
    override_dir: Option<PathBuf>,
    local: Option<PathBuf>,
    roaming: Option<PathBuf>,
) -> PathBuf {
    override_dir
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| {
            [local, roaming]
                .into_iter()
                .flatten()
                .find(|path| path.is_absolute())
                .map(|path| path.join("Surco"))
        })
        .unwrap_or_else(|| root.join("data"))
}

pub fn data(root: &Path) -> PathBuf {
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    data_path(root, var("SURCO_DATA"), var("LOCALAPPDATA"), var("APPDATA"))
}

pub fn profile_id(dir: &Path) -> Result<String> {
    let path = std::path::absolute(dir)?;
    let path = path.canonicalize().unwrap_or(path);
    let name = path.to_string_lossy();
    #[cfg(windows)]
    let name = name.to_lowercase();
    Ok(format!("{:x}", Sha256::digest(name.as_bytes())))
}

pub fn job_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("jobs").join(format!("{id}.json"))
}
pub fn job(dir: &Path, id: &str) -> Result<Job> {
    anyhow::ensure!(valid_id(id), "Trabajo inválido");
    read(&job_path(dir, id))
}

pub fn jobs(dir: &Path) -> Result<Vec<Job>> {
    let dir = dir.join("jobs");
    fs::create_dir_all(&dir)?;
    let mut jobs = Vec::new();
    for file in fs::read_dir(dir)? {
        let file = file?;
        let path = file.path();
        if !file.file_type()?.is_file() || path.extension().and_then(|x| x.to_str()) != Some("json")
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                eprintln!("No se pudo leer {}: {error}", path.display());
                continue;
            }
        };
        match serde_json::from_slice::<Job>(&bytes) {
            Ok(job)
                if path.file_stem().and_then(|stem| stem.to_str()) == Some(&job.id)
                    && valid_id(&job.id) =>
            {
                jobs.push(job)
            }
            _ => {
                if let Err(error) = quarantine(&path, &bytes) {
                    eprintln!("No se pudo apartar {}: {error:#}", path.display());
                }
            }
        }
    }
    jobs.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(jobs)
}

fn quarantine(path: &Path, bytes: &[u8]) -> Result<()> {
    // Serialize quarantines triggered by concurrent reads. Keep the original
    // until a complete, uniquely named copy is on disk.
    static QUARANTINE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = QUARANTINE.lock().unwrap_or_else(|error| error.into_inner());
    if !fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    let mut copy = tempfile::NamedTempFile::new_in(path.parent().context("Ruta inválida")?)?;
    copy.write_all(bytes)?;
    copy.as_file().sync_all()?;
    let preferred = path.with_extension("json.corrupt");
    match copy.persist_noclobber(&preferred) {
        Ok(_) => {}
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let suffix = format!("json.{}.corrupt", uuid::Uuid::new_v4().simple());
            error
                .file
                .persist_noclobber(path.with_extension(suffix))
                .map_err(|e| e.error)?;
        }
        Err(error) => return Err(error.error.into()),
    }
    fs::remove_file(path)?;
    Ok(())
}

pub fn job_warnings(dir: &Path) -> Result<Vec<String>> {
    let dir = dir.join("jobs");
    let mut count = 0;
    for file in fs::read_dir(dir)? {
        let file = file?;
        let path = file.path();
        if path.extension().is_some_and(|ext| ext == "corrupt")
            || (file.file_type()?.is_file()
                && path.extension().is_some_and(|ext| ext == "json")
                && !read::<Job>(&path).is_ok_and(|job| {
                    path.file_stem().and_then(|stem| stem.to_str()) == Some(&job.id)
                        && valid_id(&job.id)
                }))
        {
            count += 1;
        }
    }
    Ok(if count == 0 {
        Vec::new()
    } else {
        vec![format!(
            "Se omitieron {count} trabajo(s) ilegibles o incompatibles. Revisa la carpeta jobs del perfil."
        )]
    })
}

pub fn current(mut j: Job) -> Job {
    for item in &mut j.items {
        if item.status == "saved" && !Path::new(&item.path).is_file() {
            item.status = "missing".into();
            item.detail = "El archivo ya no está en la ruta registrada.".into();
        }
    }
    if j.status == "completed" && j.items.iter().any(|i| i.status != "saved") {
        j.status = "attention".into();
    }
    j
}
pub fn destination(config: &Config, album: &Album, track: &Track) -> Result<PathBuf> {
    let base = Path::new(&config.directory);
    let artist = safe_name(&album.artist);
    let title = safe_name(&if album.year.trim().is_empty() {
        album.title.clone()
    } else {
        format!("{} - {}", album.year, album.title)
    });
    let song = safe_name(&track.title);
    let prefix = format!("{:02} - ", track.number);
    let suffix = format!(" [{}].{}", track.video_id, config.format);
    let length = |value: &str| value.encode_utf16().count();
    let budget = 240usize.saturating_sub(length(&base.to_string_lossy()) + 3);
    // Reserve space for the job ID, track ID and yt-dlp's temporary suffixes.
    let work_space = 112;
    let fit = |value: &str, max: usize| {
        if length(value) <= max {
            return value.to_owned();
        }
        let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
        let mut taken = 0;
        let mut shortened = String::new();
        for letter in value.chars() {
            taken += letter.len_utf16();
            if taken + 9 > max {
                break;
            }
            shortened.push(letter);
        }
        format!(
            "{}-{}",
            shortened.trim_end_matches([' ', '.']),
            &digest[..8]
        )
    };
    let fixed = length(&prefix) + length(&suffix);
    let mut budgets = [length(&artist), length(&title)];
    anyhow::ensure!(
        budget >= work_space + 20 && budget >= fixed + 30,
        "La carpeta de música es demasiado larga. Elige una ruta más corta en Ajustes."
    );
    while budgets[0] + budgets[1] + work_space.max(fixed + 10) > budget {
        let index = usize::from(budgets[1] > budgets[0]);
        budgets[index] -= 1;
    }
    let song_budget = budget - budgets[0] - budgets[1] - fixed;
    Ok(base
        .join(fit(&artist, budgets[0]))
        .join(fit(&title, budgets[1]))
        .join(format!("{prefix}{}{suffix}", fit(&song, song_budget))))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn album() -> Album {
        serde_json::from_str(include_str!("../tests/fixtures/beatles-manifest.json")).unwrap()
    }
    #[test]
    fn profile_paths_prefer_override_and_windows_local_data() {
        let root = Path::new("project");
        let roaming = std::env::temp_dir().join("roaming");
        let local = std::env::temp_dir().join("local");
        let override_dir = PathBuf::from("isolated");
        assert_eq!(
            data_path(
                root,
                Some(override_dir.clone()),
                Some(local.clone()),
                Some(roaming.clone())
            ),
            override_dir
        );
        assert_eq!(
            data_path(root, None, Some(local.clone()), Some(roaming.clone())),
            local.join("Surco")
        );
        assert_eq!(
            data_path(root, None, None, Some(roaming.clone())),
            roaming.join("Surco")
        );
        assert_eq!(data_path(root, None, None, None), root.join("data"));
    }
    #[test]
    fn corrupt_jobs_are_quarantined_without_overwriting_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let good = Job {
            id: "valid".into(),
            album: album(),
            config: Config {
                directory: String::new(),
                format: "mp3".into(),
                browser: String::new(),
            },
            status: "completed".into(),
            items: vec![],
        };
        save(&job_path(dir.path(), &good.id), &good).unwrap();
        let bad = job_path(dir.path(), "broken");
        fs::write(&bad, b"broken").unwrap();
        fs::write(bad.with_extension("json.corrupt"), b"old").unwrap();
        assert_eq!(jobs(dir.path()).unwrap()[0].id, good.id);
        assert!(!bad.exists());
        assert_eq!(
            fs::read(bad.with_extension("json.corrupt")).unwrap(),
            b"old"
        );
        assert!(job_warnings(dir.path()).unwrap()[0].contains("2 trabajo"));
    }
    #[test]
    fn destination_keeps_album_folder_and_track_identity() {
        let config = Config {
            directory: "music".into(),
            format: "mp3".into(),
            browser: String::new(),
        };
        let mut album = album();
        album.year.clear();
        let mut track = album.tracks[0].clone();
        assert_eq!(
            destination(&config, &album, &track)
                .unwrap()
                .parent()
                .unwrap()
                .file_name()
                .unwrap(),
            safe_name(&album.title).as_str()
        );
        album.artist = "á".repeat(100);
        album.title = "🌊".repeat(100);
        let short = destination(&config, &album, &track).unwrap();
        track.title = "🎶".repeat(100);
        let long = destination(&config, &album, &track).unwrap();
        assert_eq!(short.parent(), long.parent());
        assert!(long.to_string_lossy().encode_utf16().count() <= 240);
        assert!(
            long.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&format!("[{}].mp3", track.video_id))
        );
    }
    #[test]
    fn atomic_replacement_is_readable() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        save(&p, &vec![1]).unwrap();
        save(&p, &vec![2, 3]).unwrap();
        assert_eq!(read::<Vec<i32>>(&p).unwrap(), vec![2, 3]);
    }
    #[test]
    fn hash_changes_with_content() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        fs::write(&p, b"a").unwrap();
        let h = hash(&p).unwrap();
        fs::write(&p, b"b").unwrap();
        assert_ne!(h, hash(&p).unwrap());
    }
}

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
pub fn data(root: &Path) -> PathBuf {
    std::env::var_os("SURCO_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data"))
}
pub fn job_path(root: &Path, id: &str) -> PathBuf {
    data(root).join("jobs").join(format!("{id}.json"))
}
pub fn job(root: &Path, id: &str) -> Result<Job> {
    anyhow::ensure!(valid_id(id), "Trabajo inválido");
    read(&job_path(root, id))
}
pub fn jobs(root: &Path) -> Result<Vec<Job>> {
    let dir = data(root).join("jobs");
    fs::create_dir_all(&dir)?;
    let mut jobs = Vec::new();
    for f in fs::read_dir(dir)? {
        let p = f?.path();
        if p.extension().and_then(|x| x.to_str()) == Some("json") {
            jobs.push(read::<Job>(&p)?);
        }
    }
    jobs.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(jobs)
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
pub fn destination(config: &Config, album: &Album, t: &Track) -> PathBuf {
    Path::new(&config.directory)
        .join(safe_name(&album.artist))
        .join(safe_name(&format!("{} - {}", album.year, album.title)))
        .join(format!(
            "{:02} - {} [{}].{}",
            t.number,
            safe_name(&t.title),
            t.video_id,
            config.format
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
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

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub number: usize,
    #[serde(default = "one")]
    pub disc: usize,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub video_id: String,
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub source_album: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub unavailable: bool,
    #[serde(default)]
    pub reason: String,
}
fn one() -> usize {
    1
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edition {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub playlist_id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    #[serde(default)]
    pub image: String,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub other_versions: Vec<Edition>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub directory: String,
    pub format: String,
    #[serde(default)]
    pub browser: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub track: Track,
    pub status: String,
    pub detail: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub backup: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub album: Album,
    pub config: Config,
    pub status: String,
    pub items: Vec<Outcome>,
}

pub fn key(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 180
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
pub fn validate_album(a: &Album) -> anyhow::Result<()> {
    anyhow::ensure!(
        a.id.starts_with("MPRE") && valid_id(&a.id),
        "El enlace no identifica un álbum."
    );
    anyhow::ensure!(
        a.playlist_id.starts_with("OLAK5uy_") && valid_id(&a.playlist_id),
        "No se encontró la lista de audio del álbum."
    );
    anyhow::ensure!(!a.tracks.is_empty(), "El álbum no contiene pistas.");
    for (i, t) in a.tracks.iter().enumerate() {
        anyhow::ensure!(t.number == i + 1, "El orden del álbum no pudo comprobarse.");
        if !t.unavailable {
            anyhow::ensure!(
                t.source_type == "MUSIC_VIDEO_TYPE_ATV",
                "Se rechazó una fuente que no es audio oficial."
            );
            anyhow::ensure!(
                t.video_id.len() == 11 && valid_id(&t.video_id),
                "Fuente inválida."
            );
        }
    }
    Ok(())
}
pub fn validate_source(t: &Track, source: &serde_json::Value) -> anyhow::Result<()> {
    anyhow::ensure!(!t.unavailable, "{}", t.reason);
    anyhow::ensure!(
        source["video_id"].as_str() == Some(t.video_id.as_str()),
        "La fuente cambió. No se descargó otra grabación."
    );
    anyhow::ensure!(
        source["source_type"] == "MUSIC_VIDEO_TYPE_ATV",
        "La fuente no se confirmó como audio oficial. No se usará un videoclip."
    );
    anyhow::ensure!(
        key(source["title"].as_str().unwrap_or("")) == key(&t.title),
        "La fuente no corresponde al título y edición de esta pista."
    );
    anyhow::ensure!(
        key(source["artist"].as_str().unwrap_or("")) == key(&t.artist),
        "La fuente no corresponde al artista de esta pista."
    );
    if let Some(duration) = source["duration"].as_f64().filter(|d| *d > 0.) {
        anyhow::ensure!(
            duration_matches(t.duration, duration),
            "La duración de la fuente difiere de la pista del álbum."
        );
    }
    Ok(())
}

pub fn duration_matches(expected: f64, actual: f64) -> bool {
    actual.is_finite() && actual > 0. && (expected <= 0. || (actual - expected).abs() <= 3.)
}
pub fn safe_name(value: &str) -> String {
    let value: String = value
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    let value = value.trim().trim_end_matches(['.', ' ']);
    let value = if value.is_empty() {
        "Sin nombre"
    } else {
        value
    };
    let stem = value.split('.').next().unwrap_or("").to_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
    {
        format!("_{value}")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editions_are_distinct() {
        assert_ne!(
            key("Norwegian Wood (2026 Mix)"),
            key("Norwegian Wood (Remastered 2009)")
        );
    }
    #[test]
    fn parentheses_are_not_removed() {
        assert_ne!(
            key("Norwegian Wood (This Bird Has Flown)"),
            key("Norwegian Wood")
        );
    }
    #[test]
    fn safe_windows_paths() {
        assert_eq!(safe_name("../CON:"), ".._CON_");
        assert_eq!(safe_name("NUL.txt"), "_NUL.txt");
    }
    #[test]
    fn videos_are_rejected() {
        let t = Track {
            number: 1,
            disc: 1,
            title: "Song".into(),
            artist: "Artist".into(),
            video_id: "abcdefghijk".into(),
            source_type: "MUSIC_VIDEO_TYPE_ATV".into(),
            source_album: "Another album".into(),
            duration: 180.,
            unavailable: false,
            reason: String::new(),
        };
        let mut s = serde_json::json!({"video_id":"abcdefghijk","title":"Song","artist":"Artist","source_type":"MUSIC_VIDEO_TYPE_OMV"});
        assert!(validate_source(&t, &s).is_err());
        s["source_type"] = "MUSIC_VIDEO_TYPE_ATV".into();
        assert!(validate_source(&t, &s).is_ok());
        s["duration"] = 821.into();
        assert!(validate_source(&t, &s).is_err());
        s["duration"] = 181.into();
        assert!(validate_source(&t, &s).is_ok());
        s["title"] = "Song (Live)".into();
        assert!(validate_source(&t, &s).is_err());
    }
}

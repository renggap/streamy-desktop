use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub duration_secs: f64,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            tracks: vec![],
            current: None,
        }
    }
    pub fn add(&mut self, t: Track) {
        // Skip tracks already in the list (same file path). Drag-dropping a
        // folder twice should not double the playlist.
        if self.tracks.iter().any(|x| x.path == t.path) {
            return;
        }
        self.tracks.push(t);
    }
    pub fn next_index(&self, current: Option<usize>) -> Option<usize> {
        match current {
            Some(i) if i + 1 < self.tracks.len() => Some(i + 1),
            _ => None,
        }
    }

    /// Persist tracks to <data_dir>/playlist.json so the playlist survives
    /// restarts. Best-effort: failures log and return Err for caller to log.
    pub fn save_to(&self, dir: &std::path::Path) -> Result<(), String> {
        crate::diag::write_atomic(
            &dir.join("playlist.json"),
            &serde_json::to_string_pretty(&self.tracks).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    /// Load tracks from <data_dir>/playlist.json. Missing/corrupt file = empty
    /// playlist (same recovery path as settings.json).
    pub fn load_from(dir: &std::path::Path) -> Self {
        let tracks = fs::read_to_string(dir.join("playlist.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| {
                if dir.join("playlist.json").exists() {
                    crate::diag::log("WARN: playlist.json corrupt; starting empty");
                }
                vec![]
            });
        Self {
            tracks,
            current: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tr(i: usize) -> Track {
        Track {
            path: format!("/t{i}.wav"),
            title: format!("t{i}"),
            artist: "a".into(),
            duration_secs: 1.0,
        }
    }

    #[test]
    fn next_stops_at_end() {
        let mut p = Playlist::new();
        p.add(tr(0));
        p.add(tr(1));
        assert_eq!(p.next_index(Some(1)), None);
    }

    #[test]
    fn playlist_roundtrips_through_json() {
        let dir = std::env::temp_dir().join(format!("streamy-pl-test-{}", std::process::id()));
        let mut p = Playlist::new();
        p.add(tr(0));
        p.add(tr(1));
        p.save_to(&dir).expect("save");
        let p2 = Playlist::load_from(&dir);
        assert_eq!(p2.tracks.len(), 2);
        assert_eq!(p2.tracks[1].path, "/t1.wav");
        assert_eq!(p2.current, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_playlist_file_loads_empty() {
        let p = Playlist::load_from(std::path::Path::new("/nonexistent-streamy-dir"));
        assert!(p.tracks.is_empty());
    }

    #[test]
    fn wrong_shape_playlist_json_loads_empty() {
        // Valid JSON, wrong shape (bare array of strings) must not panic.
        let dir = std::env::temp_dir().join(format!("streamy-pl-shape-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("playlist.json"), r#"["a.mp3","b.mp3"]"#).unwrap();
        let p = Playlist::load_from(&dir);
        assert!(p.tracks.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_skips_duplicate_paths() {
        let mut p = Playlist::new();
        let t = || Track {
            path: "/a.mp3".into(),
            title: "a".into(),
            artist: String::new(),
            duration_secs: 1.0,
        };
        p.add(t());
        p.add(t());
        assert_eq!(p.tracks.len(), 1, "duplicate path must not double-add");
    }

    #[test]
    fn unknown_fields_dropped_on_save() {
        // Old/newer writer added extra keys: load tolerates them, save writes
        // canonical schema (serde drops unknown fields on deserialize).
        let dir = std::env::temp_dir().join(format!("streamy-pl-extra-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("playlist.json"),
            r#"[{"path":"/a.mp3","title":"a","artist":"x","duration_secs":1.0,"future":"f"}]"#,
        )
        .unwrap();
        let p = Playlist::load_from(&dir);
        assert_eq!(p.tracks.len(), 1, "extras must not block load");
        p.save_to(&dir).expect("save");
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("playlist.json")).unwrap())
                .expect("saved file is valid JSON");
        let obj = saved.as_array().unwrap()[0].as_object().unwrap();
        assert!(
            !obj.contains_key("future"),
            "unknown field must be stripped"
        );
        assert!(obj.contains_key("path") && obj.contains_key("title"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

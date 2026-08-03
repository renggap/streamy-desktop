use serde::{Deserialize, Serialize};

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

impl Playlist {
    pub fn new() -> Self {
        Self {
            tracks: vec![],
            current: None,
        }
    }
    pub fn add(&mut self, t: Track) {
        self.tracks.push(t);
    }
    pub fn next_index(&self, current: Option<usize>) -> Option<usize> {
        match current {
            Some(i) if i + 1 < self.tracks.len() => Some(i + 1),
            _ => None,
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
}

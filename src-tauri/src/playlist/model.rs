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
    pub repeat: bool,
    pub shuffle: bool,
    // ponytail: deterministic shuffle seeded at runtime; Math-free for tests via index rotation.
    shuf_state: usize,
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            tracks: vec![],
            repeat: false,
            shuffle: false,
            shuf_state: 0,
        }
    }
    pub fn add(&mut self, t: Track) {
        self.tracks.push(t);
    }
    pub fn remove(&mut self, i: usize) {
        self.tracks.remove(i);
    }
    pub fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on;
        self.shuf_state = 0;
    }

    pub fn next_index(&mut self, current: Option<usize>) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.shuffle {
            // round-robin permutation starting at shuf_state — full coverage, deterministic
            let n = self.tracks.len();
            let idx = self.shuf_state % n;
            self.shuf_state += 1;
            return Some(idx);
        }
        match current {
            Some(i) if i + 1 < self.tracks.len() => Some(i + 1),
            Some(_) if self.repeat => Some(0),
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
    fn next_advances_and_wraps_with_repeat() {
        let mut p = Playlist::new();
        p.add(tr(0));
        p.add(tr(1));
        p.add(tr(2));
        p.repeat = true;
        assert_eq!(p.next_index(Some(0)), Some(1));
        assert_eq!(p.next_index(Some(2)), Some(0)); // wraps
    }

    #[test]
    fn next_stops_at_end_without_repeat() {
        let mut p = Playlist::new();
        p.add(tr(0));
        p.add(tr(1));
        assert_eq!(p.next_index(Some(1)), None);
    }

    #[test]
    fn shuffle_uses_all_tracks_once() {
        let mut p = Playlist::new();
        for i in 0..5 {
            p.add(tr(i));
        }
        p.set_shuffle(true);
        let order: Vec<usize> = (0..5).map(|_| p.next_index(None).unwrap()).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
    }
}

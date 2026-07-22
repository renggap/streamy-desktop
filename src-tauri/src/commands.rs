use std::sync::Mutex;

use tauri::{AppHandle, State};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;

use crate::engine::decoder::decode_file;
use crate::engine::engine::AudioEngine;
use crate::events;
use crate::playlist::model::{Playlist, Track};

/// Read (title, artist, duration_secs) from tags; fall back to filename + 0.
fn read_tags(path: &str) -> (String, String, f64) {
    let filename = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    match lofty::read_from_path(path) {
        Ok(tf) => {
            let tag = tf.primary_tag();
            let title = tag
                .and_then(|t| t.title().map(|s| s.to_string()))
                .unwrap_or_else(|| filename.clone());
            let artist = tag
                .and_then(|t| t.artist().map(|s| s.to_string()))
                .unwrap_or_default();
            let dur = tf.properties().duration().as_secs_f64();
            (title, artist, dur)
        }
        Err(_) => (filename, String::new(), 0.0),
    }
}

/// Decode + load a track into the deck, start the stream, emit now_playing.
fn play_at(engine: &AudioEngine, app: &AppHandle, track: &Track) -> Result<(), String> {
    let (samples, _sr, channels) = decode_file(&track.path)?;
    let deck = engine.deck();
    let mut d = deck.lock().map_err(|e| e.to_string())?;
    d.load(samples, channels);
    d.playing = true;
    drop(d);
    engine.start()?;
    events::emit_now_playing(app, track)
}

#[tauri::command]
pub fn add_to_playlist(
    paths: Vec<String>,
    playlist: State<'_, Mutex<Playlist>>,
) -> Result<Vec<Track>, String> {
    let mut p = playlist.lock().map_err(|e| e.to_string())?;
    for path in paths {
        let (title, artist, duration_secs) = read_tags(&path);
        p.add(Track {
            path,
            title,
            artist,
            duration_secs,
        });
    }
    Ok(p.tracks.clone())
}

#[tauri::command]
pub fn play_index(
    i: usize,
    playlist: State<'_, Mutex<Playlist>>,
    engine: State<'_, AudioEngine>,
    app: AppHandle,
) -> Result<(), String> {
    let track = {
        let mut p = playlist.lock().map_err(|e| e.to_string())?;
        let t = p.tracks.get(i).cloned().ok_or("index out of range")?;
        p.current = Some(i);
        t
    };
    play_at(&engine, &app, &track)
}

#[tauri::command]
pub fn next(
    playlist: State<'_, Mutex<Playlist>>,
    engine: State<'_, AudioEngine>,
    app: AppHandle,
) -> Result<(), String> {
    let track = {
        let mut p = playlist.lock().map_err(|e| e.to_string())?;
        let cur = p.current;
        let nxt = p.next_index(cur);
        p.current = nxt;
        match nxt {
            Some(i) => p.tracks.get(i).cloned(),
            None => None,
        }
    };
    match track {
        Some(t) => play_at(&engine, &app, &t),
        None => Ok(()), // end of playlist, no repeat
    }
}

#[tauri::command]
pub fn prev(
    playlist: State<'_, Mutex<Playlist>>,
    engine: State<'_, AudioEngine>,
    app: AppHandle,
) -> Result<(), String> {
    let track = {
        let p = playlist.lock().map_err(|e| e.to_string())?;
        let i = match p.current {
            Some(i) if i > 0 => Some(i - 1),
            _ => None,
        };
        match i {
            Some(j) => p.tracks.get(j).cloned(),
            None => None,
        }
    };
    if let Some(t) = track {
        let mut p = playlist.lock().map_err(|e| e.to_string())?;
        if let Some(j) = p.current {
            if j > 0 {
                p.current = Some(j - 1);
            }
        }
        drop(p);
        return play_at(&engine, &app, &t);
    }
    Ok(())
}

#[tauri::command]
pub fn pause(engine: State<'_, AudioEngine>) -> Result<(), String> {
    let deck = engine.deck();
    let mut d = deck.lock().map_err(|e| e.to_string())?;
    d.playing = false;
    Ok(())
}

#[tauri::command]
pub fn seek(pct: f64, engine: State<'_, AudioEngine>) -> Result<(), String> {
    let deck = engine.deck();
    let mut d = deck.lock().map_err(|e| e.to_string())?;
    let c = d.channels.max(1);
    let total = d.samples.len();
    let pos = ((pct.clamp(0.0, 1.0)) * (total as f64)) as usize;
    d.pos = (pos / c) * c; // align to frame boundary
    Ok(())
}

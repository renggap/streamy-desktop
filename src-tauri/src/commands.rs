use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, State};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use rayon::prelude::*;

use crate::engine::engine::AudioEngine;
use crate::engine::mixer::SourceMode;
use crate::events;
use crate::playlist::model::{Playlist, Track};

/// Read (title, artist, duration_secs) from tags; fall back to filename + 0.
/// Skips cover-art parsing (we don't display it) — the expensive part on files
/// with large embedded art / under AV-scanned I/O.
fn read_tags(path: &str) -> (String, String, f64) {
    let filename = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let opts = ParseOptions::new().read_cover_art(false);
    match Probe::open(path).and_then(|p| p.options(opts).read()) {
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

/// Hand a track to the audio thread (streams from disk, starts in ms) and
/// emit now_playing. Non-blocking — decode is incremental on the audio thread.
fn play_at(engine: &AudioEngine, app: &AppHandle, track: &Track) -> Result<(), String> {
    engine.load(track.path.clone())?;
    events::emit_now_playing(app, track)
}

#[tauri::command]
pub fn add_to_playlist(
    paths: Vec<String>,
    playlist: State<'_, Mutex<Playlist>>,
) -> Result<Vec<Track>, String> {
    // Parse tags in parallel — tag reads are I/O bound (and on Windows, each
    // open is contended by Defender), so a parallel map is a large win.
    let parsed: Vec<Track> = paths
        .par_iter()
        .map(|path| {
            let (title, artist, duration_secs) = read_tags(path);
            Track {
                path: path.clone(),
                title,
                artist,
                duration_secs,
            }
        })
        .collect();
    let mut p = playlist.lock().map_err(|e| e.to_string())?;
    for t in parsed {
        p.add(t);
    }
    Ok(p.tracks.clone())
}

/// Snapshot used to restore UI state after a webview reload (right-click refresh).
#[derive(Serialize)]
pub struct PlaylistSnapshot {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
}

#[tauri::command]
pub fn list_playlist(playlist: State<'_, Mutex<Playlist>>) -> Result<PlaylistSnapshot, String> {
    let p = playlist.lock().map_err(|e| e.to_string())?;
    Ok(PlaylistSnapshot {
        tracks: p.tracks.clone(),
        current: p.current,
    })
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
        // Keep current at the last valid index even at the end of the list,
        // so Prev still works after Next hits the end.
        if let Some(i) = nxt {
            p.current = Some(i);
        }
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
pub fn play(engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.play()
}

#[tauri::command]
pub fn pause(engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.pause()
}

#[tauri::command]
pub fn seek(pct: f64, engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.seek(pct)
}

#[tauri::command]
pub fn set_source_mode(mode: SourceMode, engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.set_source_mode(mode)
}

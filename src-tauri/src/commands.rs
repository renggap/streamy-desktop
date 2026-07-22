use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use rayon::prelude::*;

use crate::diag;
use crate::engine::device;
use crate::engine::engine::AudioEngine;
use crate::engine::mixer::SourceMode;
use crate::encode::{Codec, EncoderConfig};
use crate::events;
use crate::playlist::model::{Playlist, Track};
use crate::profile::store::{ConnectionProfile, ProfileStore};
use crate::record::{RecFormat, RecorderHandle, RecordingStatus};
use crate::settings::{Settings, SettingsStore};
use crate::stream::broadcast::{self, BroadcastHandle, StreamStatus};
use crate::stream::ultravox::UltravoxConfig;

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
    diag::log(format!("seek -> {pct}"));
    engine.seek(pct)
}

#[tauri::command]
pub fn set_source_mode(mode: SourceMode, engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.set_source_mode(mode)
}

#[tauri::command]
pub fn set_crossfade(pos: f32, engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.set_crossfade(pos)
}

#[tauri::command]
pub fn set_eq(low_db: f64, mid_db: f64, high_db: f64, engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.set_eq(low_db, mid_db, high_db)
}

#[tauri::command]
pub fn set_duck(enabled: bool, threshold: f32, depth_db: f32, engine: State<'_, AudioEngine>) -> Result<(), String> {
    engine.set_duck(enabled, threshold, depth_db)
}

#[tauri::command]
pub fn list_input_devices() -> Vec<device::DeviceInfo> {
    device::list_input_devices()
}

#[tauri::command]
pub fn list_output_devices() -> Vec<device::DeviceInfo> {
    device::list_output_devices()
}

#[tauri::command]
pub fn set_input_device(
    name: String,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    diag::log(format!("set_input_device -> '{name}'"));
    engine.set_input_device(name.clone())?;
    settings.set_input_device(name)
}

#[tauri::command]
pub fn get_settings(settings: State<'_, SettingsStore>) -> Settings {
    settings.get()
}

// --- P2.4/2.5 connection profiles ---

#[tauri::command]
pub fn list_profiles(store: State<'_, ProfileStore>) -> Vec<ConnectionProfile> {
    store.list()
}

#[tauri::command]
pub fn save_profile(
    profile: ConnectionProfile,
    password: Option<String>,
    store: State<'_, ProfileStore>,
) -> Result<(), String> {
    store.save(profile, password)
}

#[tauri::command]
pub fn delete_profile(name: String, store: State<'_, ProfileStore>) -> Result<(), String> {
    store.delete(&name)
}

// --- P2.2 connect / disconnect (Ultravox broadcast) ---

#[tauri::command]
pub async fn connect_stream(
    name: String,
    bitrate_kbps: u32,
    codec: Codec,
    engine: State<'_, AudioEngine>,
    profiles: State<'_, ProfileStore>,
    bcast: State<'_, Mutex<Option<BroadcastHandle>>>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let g = bcast.lock().map_err(|e| e.to_string())?;
        if g.is_some() {
            return Err("already connected".into());
        }
    }
    diag::log(format!("connect_stream -> '{name}' @ {bitrate_kbps} kbps {codec:?}"));
    let profile = profiles
        .list()
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("no profile named {name}"))?;
    let password = profiles.get_password(&name)?;
    let content_type = match codec {
        Codec::Mp3 => "audio/mpeg",
        Codec::Aac => "audio/aacp",
    };
    let uvox_cfg = UltravoxConfig {
        host: profile.host,
        port: profile.port,
        stream_id: profile.stream_id,
        password,
        content_type: content_type.into(),
        bitrate_kbps,
        name: profile.station_name,
        genre: profile.genre,
        url: profile.url,
    };
    let enc_cfg = EncoderConfig {
        sample_rate: engine.output_rate(),
        channels: 2,
        bitrate_kbps,
        codec,
    };

    let (tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(16384);
    engine.set_broadcast_producer(Some(tx));
    match broadcast::start(rx, uvox_cfg, enc_cfg, Some(app)).await {
        Ok(h) => {
            if let Ok(mut g) = bcast.lock() {
                *g = Some(h);
            }
            Ok(())
        }
        Err(e) => {
            engine.set_broadcast_producer(None);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn disconnect_stream(
    engine: State<'_, AudioEngine>,
    bcast: State<'_, Mutex<Option<BroadcastHandle>>>,
    app: AppHandle,
) -> Result<(), String> {
    let h = bcast.lock().map_err(|e| e.to_string())?.take();
    if let Some(h) = h {
        h.stop().await;
    }
    engine.set_broadcast_producer(None);
    let _ = app.emit(events::STREAM_STATUS, StreamStatus::Disconnected);
    Ok(())
}

#[tauri::command]
pub async fn set_stream_title(
    title: String,
    bcast: State<'_, Mutex<Option<BroadcastHandle>>>,
) -> Result<(), String> {
    let tx = bcast
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .map(|h| h.metadata_tx.clone());
    if let Some(tx) = tx {
        tx.send(title).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

// --- P3 recording ---

pub struct RecSlot {
    handle: RecorderHandle,
    path: String,
}

#[tauri::command]
pub async fn start_recording(
    format: RecFormat,
    engine: State<'_, AudioEngine>,
    rec: State<'_, Mutex<Option<RecSlot>>>,
    app: AppHandle,
) -> Result<String, String> {
    {
        let g = rec.lock().map_err(|e| e.to_string())?;
        if g.is_some() {
            return Err("already recording".into());
        }
    }
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let rec_dir = data_dir.join("recordings");
    std::fs::create_dir_all(&rec_dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let ext = match format {
        RecFormat::Mp3 => "mp3",
        RecFormat::Wav => "wav",
    };
    let path = rec_dir.join(format!("streamy-{stamp}.{ext}"));
    let path_str = path.to_string_lossy().into_owned();
    diag::log(format!("start_recording -> {format:?} {path_str}"));

    let rate = engine.output_rate();
    let enc_cfg = EncoderConfig {
        sample_rate: rate,
        channels: 2,
        bitrate_kbps: 192,
        codec: Codec::Mp3,
    };
    let (tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(16384);
    engine.set_record_producer(Some(tx));

    // Surface disk-full / write errors immediately via the event.
    let app_for_err = app.clone();
    let on_error: Box<dyn FnOnce(String) + Send> = Box::new(move |msg| {
        diag::log(format!("recording error: {msg}"));
        let _ = app_for_err.emit(
            events::RECORDING_STATUS,
            RecordingStatus::Error { message: msg },
        );
    });

    match crate::record::start_file(rx, format, enc_cfg, rate, &path_str, Some(on_error)) {
        Ok(h) => {
            if let Ok(mut g) = rec.lock() {
                *g = Some(RecSlot {
                    handle: h,
                    path: path_str.clone(),
                });
            }
            let _ = app.emit(
                events::RECORDING_STATUS,
                RecordingStatus::Recording {
                    path: path_str.clone(),
                },
            );
            Ok(path_str)
        }
        Err(e) => {
            engine.set_record_producer(None);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn stop_recording(
    engine: State<'_, AudioEngine>,
    rec: State<'_, Mutex<Option<RecSlot>>>,
    app: AppHandle,
) -> Result<(), String> {
    let slot = rec.lock().map_err(|e| e.to_string())?.take();
    engine.set_record_producer(None);
    let Some(slot) = slot else {
        return Ok(()); // not recording — no-op
    };
    let path = slot.path.clone();
    match slot.handle.stop() {
        Ok(()) => {
            let _ = app.emit(
                events::RECORDING_STATUS,
                RecordingStatus::Stopped { path },
            );
            Ok(())
        }
        Err(e) => {
            // Error event already emitted by the recorder thread's on_error hook.
            Err(e)
        }
    }
}

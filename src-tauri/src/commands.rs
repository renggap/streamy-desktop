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
use crate::stream::broadcast::{self, BroadcastHandle, StreamConfig, StreamStatus};
use crate::stream::icy::IcySourceConfig;
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
pub fn set_output_device(
    name: Option<String>,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    diag::log(format!("set_output_device -> '{name:?}'"));
    settings.set_output_device(name.clone())?;
    engine.set_output_device(name)
}

#[tauri::command]
pub fn set_recording_dir(
    dir: Option<String>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    settings.set_recording_dir(dir)
}

/// Master output volume as a percent (0..=200). Persisted + applied live.
#[tauri::command]
pub fn set_volume(
    volume_pct: f32,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    let gain = (volume_pct / 100.0).clamp(0.0, 2.0);
    settings.set_volume(gain)?;
    engine.set_volume(gain)
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
    // Auto-detect: try Ultravox (DNAS2) first, fall back to Icecast/Liquidsoap
    // harbor. The user no longer picks a protocol — connect_stream hands both
    // configs to the streamer, which latches to whichever handshake succeeds.
    let sample_rate = engine.output_rate();
    let stream_cfg = StreamConfig::Auto {
        ultravox: UltravoxConfig {
            host: profile.host.clone(),
            port: profile.port,
            stream_id: profile.stream_id,
            password: password.clone(),
            user: profile.user.clone(),
            content_type: content_type.into(),
            bitrate_kbps,
            name: profile.station_name.clone(),
            genre: profile.genre.clone(),
            url: profile.url.clone(),
        },
        // Icecast / Liquidsoap harbor: HTTP SOURCE + Basic auth (user + source
        // password); metadata over the /admin/metadata admin endpoint.
        icecast: IcySourceConfig {
            host: profile.host.clone(),
            port: profile.port,
            mount: profile.mount.clone(),
            user: profile.user.clone(),
            password,
            content_type: content_type.into(),
            icy_name: profile.station_name.clone(),
            icy_genre: profile.genre.clone(),
            icy_url: profile.url.clone(),
            icy_pub: profile.public,
            icy_br: bitrate_kbps,
            sample_rate,
        },
    };
    let enc_cfg = EncoderConfig {
        sample_rate,
        channels: 2,
        bitrate_kbps,
        codec,
    };

    let (tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(16384);
    engine.set_broadcast_producer(Some(tx));
    match broadcast::start(rx, stream_cfg, enc_cfg, Some(app)).await {
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

/// Cap a stream title at 200 chars without splitting UTF-8.
pub fn clamp_title(title: String) -> String {
    if title.chars().count() <= 200 {
        return title;
    }
    title.chars().take(200).collect()
}

/// Cheap sanity check: does this look like a Windows PE executable?
/// Verifies the MZ header and the PE\0\0 signature at the e_lfanew offset.
pub fn looks_like_pe_exe(bytes: &[u8]) -> bool {
    if bytes.len() < 2 || bytes[0] != b'M' || bytes[1] != b'Z' {
        return false;
    }
    // Read e_lfanew at 0x3C (u32 LE) — the offset to the PE signature.
    if bytes.len() < 64 {
        return false;
    }
    let off = u32::from_le_bytes([bytes[0x3C], bytes[0x3D], bytes[0x3E], bytes[0x3F]]) as usize;
    // Bounds-check: the 4-byte PE signature must fit.
    if off + 4 > bytes.len() {
        return false;
    }
    &bytes[off..off + 4] == b"PE\0\0"
}

#[tauri::command]
pub async fn set_stream_title(
    title: String,
    bcast: State<'_, Mutex<Option<BroadcastHandle>>>,
) -> Result<(), String> {
    let title = clamp_title(title);
    // Pull what we need under a brief lock; never await while holding it (the
    // Icecast metadata round-trip can take up to its 5s timeout).
    let (ice_admin, uvox_tx) = {
        let g = bcast.lock().map_err(|e| e.to_string())?;
        match g.as_ref() {
            Some(h) => (h.icecast_admin.clone(), Some(h.metadata_tx.clone())),
            None => (None, None),
        }
    };
    match (ice_admin, uvox_tx) {
        // Icecast: metadata is a separate admin HTTP GET — send it direct so a
        // busy or dead audio streamer can't make title updates fail with
        // "channel closed" (the streamer owns the in-band path).
        (Some(admin), _) => crate::stream::icy::send_metadata(&admin, &title).await,
        // Ultravox: in-band on the audio socket, via the streamer's channel.
        (None, Some(tx)) => tx.send(title).await.map_err(|e| e.to_string()),
        (None, None) => Ok(()), // not connected — no-op
    }
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
    settings: State<'_, SettingsStore>,
    rec: State<'_, Mutex<Option<RecSlot>>>,
    app: AppHandle,
) -> Result<String, String> {
    {
        let g = rec.lock().map_err(|e| e.to_string())?;
        if g.is_some() {
            return Err("already recording".into());
        }
    }
    // Prefer the user-chosen recording folder; fall back to <app-data>/recordings.
    let rec_dir = match settings.get().recording_dir {
        Some(d) => std::path::PathBuf::from(d),
        None => app.path().app_data_dir().map_err(|e| e.to_string())?.join("recordings"),
    };
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

// --- WebView2 runtime repair (in-app, with progress) ---

const WV2_BOOTSTRAPPER_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

#[derive(Serialize, Clone)]
struct Wv2Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Is the WebView2 runtime installed? On Windows, checks the EdgeUpdate registry
/// keys (per-machine WOW64 + native, and per-user). Non-Windows: always true.
/// Used to suppress the repair interstitial when the runtime is already present.
#[tauri::command]
pub fn webview2_installed() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_webview2_installed()
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

#[cfg(target_os = "windows")]
fn is_webview2_installed() -> bool {
    use std::process::Command;
    // Evergreen standalone runtime client id.
    const CID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let keys = [
        format!("HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{CID}"),
        format!("HKLM\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{CID}"),
        format!("HKCU\\Software\\Microsoft\\EdgeUpdate\\Clients\\{CID}"),
    ];
    for key in keys {
        let out = Command::new("reg").args(["query", &key, "/v", "pv"]).output();
        if let Ok(out) = out {
            if !out.status.success() {
                continue;
            }
            // Key exists; pv = "0.0.0.0" is the not-installed placeholder.
            let s = String::from_utf8_lossy(&out.stdout);
            if !s.contains("0.0.0.0") {
                return true;
            }
        }
    }
    false
}

/// Download the official WebView2 bootstrapper (small, ~2 MB) with live progress
/// events, then run it so Microsoft's installer can install/repair the runtime.
/// The installer runs with its own UI (and UAC) so the user sees the runtime
/// download/install progress.
#[tauri::command]
pub async fn repair_webview2(app: AppHandle) -> Result<String, String> {
    diag::log("repair_webview2 -> downloading bootstrapper".to_string());
    let resp = reqwest::get(WV2_BOOTSTRAPPER_URL)
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("download failed: {e}"))?;
    let total = resp.content_length();
    let mut resp = resp;

    let mut buf: Vec<u8> = Vec::new();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("download failed: {e}"))?
    {
        buf.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;
        let _ = app.emit(
            events::WEBVIEW2_PROGRESS,
            Wv2Progress { downloaded, total },
        );
    }

    // Integrity check: bail early if the response is not a PE executable.
    if !looks_like_pe_exe(&buf) {
        return Err("downloaded file is not a Windows executable".into());
    }

    // Unique path per call: PID + timestamp avoid races with concurrent
    // repairs or a still-sleeping cleanup thread from a prior invocation.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let out = std::env::temp_dir().join(format!(
        "streamy-webview2-setup-{}-{}.exe",
        std::process::id(),
        ts
    ));
    std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
    std::process::Command::new(&out)
        .spawn()
        .map_err(|e| format!("failed to launch installer: {e}"))?;
    let path = out.to_string_lossy().into_owned();
    diag::log(format!("repair_webview2 -> launched {path}"));

    // Best-effort cleanup after the installer UI finishes.
    // The 120 s grace period is generous — the Microsoft bootstrapper
    // typically completes (or fails) within 30–60 s.  If we race the
    // installer, the next repair call simply re-downloads the file.
    let out2 = out.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(120));
        let _ = std::fs::remove_file(&out2);
    });

    Ok(path)
}

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use rayon::prelude::*;

use crate::diag;
use crate::encode::{Codec, EncoderConfig};
use crate::engine::device;
use crate::engine::engine::AudioEngine;
use crate::engine::mixer::SourceMode;
use crate::events;
use crate::playlist::model::{Playlist, Track};
use crate::profile::store::{ConnectionProfile, ProfileStore};
use crate::update;
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
    app: AppHandle,
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
    // Persist so the playlist survives restarts. Failure is non-fatal (log only).
    if let Ok(data_dir) = app.path().app_data_dir() {
        if let Err(e) = p.save_to(&data_dir) {
            crate::diag::log(format!("WARN: playlist save failed: {e}"));
        }
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
    // Compute and advance `current` under one lock — a second lock can race a
    // concurrent next()/play_index() and play the wrong track.
    let track = {
        let mut p = playlist.lock().map_err(|e| e.to_string())?;
        let i = match p.current {
            Some(i) if i > 0 => Some(i - 1),
            _ => None,
        };
        if let Some(j) = i {
            p.current = Some(j);
        }
        i.and_then(|j| p.tracks.get(j).cloned())
    };
    if let Some(t) = track {
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
pub fn set_eq(
    low_db: f64,
    mid_db: f64,
    high_db: f64,
    engine: State<'_, AudioEngine>,
) -> Result<(), String> {
    engine.set_eq(low_db, mid_db, high_db)
}

#[tauri::command]
pub fn set_duck(
    enabled: bool,
    threshold: f32,
    depth_db: f32,
    engine: State<'_, AudioEngine>,
) -> Result<(), String> {
    engine.set_duck(enabled, threshold, depth_db)
}

/// Mic noise suppression (RNNoise) on/off. Persisted + applied live.
#[tauri::command]
pub fn set_mic_denoise(
    enabled: bool,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    diag::log(format!("set_mic_denoise -> {enabled}"));
    settings.set_mic_denoise(enabled)?;
    engine.set_denoise(enabled)
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

/// Monitor file (deck) volume as a percent (0..=200). Persisted + applied live.
#[tauri::command]
pub fn set_monitor_file_volume(
    volume_pct: f32,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
) -> Result<(), String> {
    let gain = (volume_pct / 100.0).clamp(0.0, 2.0);
    settings.set_monitor_file_volume(gain)?;
    engine.set_monitor_file_gain(gain)
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

/// Lifecycle of the broadcast connection. A plain `Option<BroadcastHandle>`
/// had a race: check(is-none) → install-producer → start → store is not
/// atomic, so a `disconnect_stream` landing mid-connect could clear the
/// producer after `connect_stream` installed it — UI stuck "ON AIR" with a
/// dead stream. `Connecting` makes the intent visible so whichever side runs
/// second cleans up.
#[allow(clippy::large_enum_variant)] // Connected handle is rare and boxed by usage
pub enum BcastState {
    Idle,
    Connecting,
    Connected(BroadcastHandle),
}

#[tauri::command]
pub async fn connect_stream(
    name: String,
    bitrate_kbps: u32,
    codec: Codec,
    engine: State<'_, AudioEngine>,
    profiles: State<'_, ProfileStore>,
    bcast: State<'_, Mutex<BcastState>>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut g = bcast.lock().map_err(|e| e.to_string())?;
        if !matches!(*g, BcastState::Idle) {
            return Err("already connected".into());
        }
        *g = BcastState::Connecting;
    }
    diag::log(format!(
        "connect_stream -> '{name}' @ {bitrate_kbps} kbps {codec:?}"
    ));
    if !engine.has_output_device() {
        let mut g = bcast.lock().map_err(|e| e.to_string())?;
        *g = BcastState::Idle;
        return Err("no output audio device: plug in / select a speaker before going live".into());
    }
    let result =
        connect_stream_inner(&name, bitrate_kbps, codec, &engine, &profiles, &bcast, &app).await;
    if result.is_err() {
        // Roll Connecting back unless a disconnect already set Idle (it then
        // owns the cleanup).
        if let Ok(mut g) = bcast.lock() {
            if matches!(*g, BcastState::Connecting) {
                *g = BcastState::Idle;
            }
        }
    }
    result
}

async fn connect_stream_inner(
    name: &str,
    bitrate_kbps: u32,
    codec: Codec,
    engine: &AudioEngine,
    profiles: &ProfileStore,
    bcast: &Mutex<BcastState>,
    app: &AppHandle,
) -> Result<(), String> {
    let profile = profiles
        .list()
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("no profile named {name}"))?;
    let password = profiles.get_password(name)?;
    let content_type = match codec {
        Codec::Mp3 => "audio/mpeg",
        Codec::Aac => "audio/aacp",
    };
    // Auto-detect: try Ultravox (DNAS2) first, fall back to Icecast/Liquidsoap
    // harbor. The user no longer picks a protocol — connect_stream hands both
    // configs to the streamer, which latches to whichever handshake succeeds.
    let sample_rate = engine.output_rate();
    // Icecast / Liquidsoap harbor: HTTP SOURCE + Basic auth (user + source
    // password); metadata over the /admin/metadata admin endpoint. Built for
    // both paths — AzuraCast uses it directly, Auto keeps it as the fallback.
    let icecast = IcySourceConfig {
        host: profile.host.clone(),
        port: profile.port,
        mount: profile.mount.clone(),
        user: profile.user.clone(),
        password: password.clone(),
        content_type: content_type.into(),
        icy_name: profile.station_name.clone(),
        icy_genre: profile.genre.clone(),
        icy_url: profile.url.clone(),
        icy_pub: profile.public,
        icy_br: bitrate_kbps,
        sample_rate,
    };
    // AzuraCast accepts the DJ/source feed over Icecast even when relaying to a
    // Shoutcast DNAS for listeners — so skip the Ultravox probe and connect
    // Icecast directly. Default (Shoutcast DNAS): Auto, Ultravox-first.
    let stream_cfg = if profile.azuracast {
        StreamConfig::Icecast(icecast)
    } else {
        StreamConfig::Auto {
            ultravox: UltravoxConfig {
                host: profile.host.clone(),
                port: profile.port,
                stream_id: profile.stream_id,
                password,
                user: profile.user.clone(),
                content_type: content_type.into(),
                bitrate_kbps,
                name: profile.station_name.clone(),
                genre: profile.genre.clone(),
                url: profile.url.clone(),
            },
            icecast,
        }
    };
    let enc_cfg = EncoderConfig {
        sample_rate,
        channels: 2,
        bitrate_kbps,
        codec,
    };

    let (tx, rx) = crate::engine::ring::StereoRing::new(16384);
    engine.set_broadcast_producer(Some(tx));
    // Forward broadcast events to the Tauri event bus. The broadcast core
    // stays UI-free (see broadcast::EventEmitter docs).
    let emitter: broadcast::EventEmitter = {
        let app = app.clone();
        std::sync::Arc::new(move |event: &str, payload: serde_json::Value| {
            use tauri::Emitter;
            let _ = app.emit(event, payload);
        })
    };
    match broadcast::start(rx, stream_cfg, enc_cfg, Some(emitter)).await {
        Ok(h) => {
            // Decide under the lock, drop it, then await. MutexGuard is not
            // Send and must never be held across an await.
            enum Commit {
                Store,
                TearDown,
            }
            let action = {
                let g = bcast.lock().map_err(|e| e.to_string())?;
                match *g {
                    // A concurrent disconnect ran while we were starting: it
                    // already cleared the producer; tear this fresh connection
                    // down instead of leaking an ON-AIR state with no consumer.
                    BcastState::Idle => Commit::TearDown,
                    // Still ours (Connecting): commit the handle.
                    BcastState::Connecting => Commit::Store,
                    // A second connect raced past the guard — shouldn't happen.
                    BcastState::Connected(_) => Commit::TearDown,
                }
            };
            match action {
                Commit::Store => {
                    let mut g = bcast.lock().map_err(|e| e.to_string())?;
                    *g = BcastState::Connected(h);
                }
                Commit::TearDown => {
                    engine.set_broadcast_producer(None);
                    h.stop().await;
                }
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
    bcast: State<'_, Mutex<BcastState>>,
    app: AppHandle,
) -> Result<(), String> {
    let h = {
        let mut g = bcast.lock().map_err(|e| e.to_string())?;
        match std::mem::replace(&mut *g, BcastState::Idle) {
            BcastState::Connected(h) => Some(h),
            other => {
                *g = other; // keep Connecting/Idle as-is
                None
            }
        }
    };
    // Clear the producer BEFORE stopping: after Disconnect returns, the audio
    // callback must not keep pushing into a dead ring.
    engine.set_broadcast_producer(None);
    if let Some(h) = h {
        h.stop().await;
    }
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
    bcast: State<'_, Mutex<BcastState>>,
) -> Result<(), String> {
    let title = clamp_title(title);
    // Pull what we need under a brief lock; never await while holding it (the
    // Icecast metadata round-trip can take up to its 5s timeout).
    let (ice_admin, uvox_tx) = {
        let g = bcast.lock().map_err(|e| e.to_string())?;
        match &*g {
            BcastState::Connected(h) => (h.icecast_admin.clone(), Some(h.metadata_tx.clone())),
            _ => (None, None),
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

/// Recording lifecycle, mirroring `BcastState`: `Starting` closes the
/// check-then-set race where two rapid `start_recording` calls both passed an
/// `is_some()` check and the second overwrote the first record producer ring
/// (first recorder then drained a dead ring forever).
pub enum RecState {
    Idle,
    Starting,
    Active(RecSlot),
}

#[tauri::command]
pub async fn start_recording(
    format: RecFormat,
    engine: State<'_, AudioEngine>,
    settings: State<'_, SettingsStore>,
    rec: State<'_, Mutex<RecState>>,
    app: AppHandle,
) -> Result<String, String> {
    {
        let mut g = rec.lock().map_err(|e| e.to_string())?;
        if !matches!(*g, RecState::Idle) {
            return Err("already recording".into());
        }
        *g = RecState::Starting;
    }
    if !engine.has_output_device() {
        let mut g = rec.lock().map_err(|e| e.to_string())?;
        *g = RecState::Idle;
        return Err("no output audio device: plug in / select a speaker before recording".into());
    }
    // Prefer the user-chosen recording folder; fall back to <app-data>/recordings.
    let rec_dir = match settings.get().recording_dir {
        Some(d) => std::path::PathBuf::from(d),
        None => app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("recordings"),
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
    let (tx, rx) = crate::engine::ring::StereoRing::new(16384);
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
            // Commit under the lock; if a concurrent stop ran during setup it
            // already cleared the producer and set Idle — honor that and tear
            // the fresh recorder down instead of leaking an Active take.
            let commit = {
                let mut g = rec.lock().map_err(|e| e.to_string())?;
                match std::mem::replace(&mut *g, RecState::Idle) {
                    RecState::Starting => false,
                    RecState::Active(slot) => {
                        *g = RecState::Active(slot);
                        false
                    }
                    RecState::Idle => true, // stop won; tear down below
                }
            };
            if commit {
                engine.set_record_producer(None);
                let _ = h.stop();
                return Ok(path_str);
            }
            *rec.lock().map_err(|e| e.to_string())? = RecState::Active(RecSlot {
                handle: h,
                path: path_str.clone(),
            });
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
            // Roll Starting back unless stop already set Idle.
            if let Ok(mut g) = rec.lock() {
                if matches!(*g, RecState::Starting) {
                    *g = RecState::Idle;
                }
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn stop_recording(
    engine: State<'_, AudioEngine>,
    rec: State<'_, Mutex<RecState>>,
    app: AppHandle,
) -> Result<(), String> {
    // Take an Active slot only; Starting stays untouched so the in-flight
    // start sees Idle at commit and cleans itself up.
    let slot = {
        let mut g = rec.lock().map_err(|e| e.to_string())?;
        match std::mem::replace(&mut *g, RecState::Idle) {
            RecState::Active(s) => Some(s),
            RecState::Starting => {
                *g = RecState::Starting;
                None
            }
            RecState::Idle => None,
        }
    };
    engine.set_record_producer(None);
    let Some(slot) = slot else {
        return Ok(()); // not recording — no-op
    };
    let path = slot.path.clone();
    // slot already taken above; join happens in handle.stop(). The recorder's
    // on_error hook may emit RECORDING_STATUS concurrently — fine, we hold no
    // lock while joining.
    match slot.handle.stop() {
        Ok(()) => {
            let _ = app.emit(events::RECORDING_STATUS, RecordingStatus::Stopped { path });
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
        let out = Command::new("reg")
            .args(["query", &key, "/v", "pv"])
            .output();
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
    diag::log("repair_webview2 -> downloading bootstrapper");
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
        let _ = app.emit(events::WEBVIEW2_PROGRESS, Wv2Progress { downloaded, total });
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

// --- Update check ---------------------------------------------------------

#[tauri::command]
pub async fn check_update() -> Result<Option<update::UpdateInfo>, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    // Fail soft: an offline/failed check must never disturb the DJ.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let latest = match update::fetch_latest(&client).await {
        Ok(i) => i,
        Err(e) => {
            diag::log(format!("update check: {e}"));
            return Ok(None);
        }
    };
    if update::is_newer(&latest.version, &current) {
        diag::log(format!(
            "update available: {} -> {}",
            current, latest.version
        ));
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

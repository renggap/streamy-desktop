use tauri::{AppHandle, Emitter};

use crate::playlist::model::Track;

pub const NOW_PLAYING: &str = "now_playing";
pub const ERROR: &str = "error";
pub const LEVELS: &str = "levels";
pub const TRACK_ENDED: &str = "track_ended";
pub const STREAM_STATUS: &str = "stream_status";
pub const POSITION: &str = "position";
pub const RECORDING_STATUS: &str = "recording_status";
pub const WEBVIEW2_PROGRESS: &str = "webview2_progress";
pub const STREAM_STATS: &str = "stream_stats";

/// Per-session network upload stats, emitted every second while streaming.
#[derive(serde::Serialize, Clone)]
pub struct StreamStats {
    /// Instant upload rate, kbit/s (1000-bit kb), over the last second.
    pub kbps: f64,
    /// Total audio payload bytes written this session (since connect).
    pub total_bytes: u64,
}

pub fn emit_now_playing(app: &AppHandle, track: &Track) -> Result<(), String> {
    app.emit(NOW_PLAYING, track).map_err(|e| e.to_string())
}

pub fn emit_error(app: &AppHandle, msg: impl Into<String>) {
    let m = msg.into();
    crate::diag::log(format!("ERROR: {m}"));
    let _ = app.emit(ERROR, m);
}

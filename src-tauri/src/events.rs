use tauri::{AppHandle, Emitter};

use crate::playlist::model::Track;

pub const NOW_PLAYING: &str = "now_playing";
pub const ERROR: &str = "error";
pub const LEVELS: &str = "levels";
pub const TRACK_ENDED: &str = "track_ended";
pub const STREAM_STATUS: &str = "stream_status";
pub const POSITION: &str = "position";

pub fn emit_now_playing(app: &AppHandle, track: &Track) -> Result<(), String> {
    app.emit(NOW_PLAYING, track).map_err(|e| e.to_string())
}

pub fn emit_error(app: &AppHandle, msg: impl Into<String>) {
    let m = msg.into();
    crate::diag::log(format!("ERROR: {m}"));
    let _ = app.emit(ERROR, m);
}

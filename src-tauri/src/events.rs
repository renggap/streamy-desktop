use tauri::{AppHandle, Emitter};

use crate::playlist::model::Track;

pub const NOW_PLAYING: &str = "now_playing";
pub const ERROR: &str = "error";

pub fn emit_now_playing(app: &AppHandle, track: &Track) -> Result<(), String> {
    app.emit(NOW_PLAYING, track).map_err(|e| e.to_string())
}

pub fn emit_error(app: &AppHandle, msg: impl Into<String>) {
    let _ = app.emit(ERROR, msg.into());
}

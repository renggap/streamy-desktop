use tauri::{AppHandle, Emitter};

use crate::playlist::model::Track;

pub const NOW_PLAYING: &str = "now_playing";

pub fn emit_now_playing(app: &AppHandle, track: &Track) -> Result<(), String> {
    app.emit(NOW_PLAYING, track).map_err(|e| e.to_string())
}

use std::sync::Mutex;

use tauri::State;

use crate::engine::decoder::decode_file;
use crate::engine::engine::AudioEngine;

#[tauri::command]
pub fn load_track(path: String, engine: State<'_, Mutex<AudioEngine>>) -> Result<(), String> {
    let (samples, _sr, channels) = decode_file(&path)?;
    let eng = engine.lock().map_err(|e| e.to_string())?;
    let deck = eng.deck();
    let mut d = deck.lock().map_err(|e| e.to_string())?;
    d.load(samples, channels);
    Ok(())
}

#[tauri::command]
pub fn play(engine: State<'_, Mutex<AudioEngine>>) -> Result<(), String> {
    let mut eng = engine.lock().map_err(|e| e.to_string())?;
    if !eng.is_running() {
        eng.start()?;
    }
    let deck = eng.deck();
    let mut d = deck.lock().map_err(|e| e.to_string())?;
    d.playing = true;
    Ok(())
}

#[tauri::command]
pub fn pause(engine: State<'_, Mutex<AudioEngine>>) -> Result<(), String> {
    let eng = engine.lock().map_err(|e| e.to_string())?;
    let deck = eng.deck();
    let mut d = deck.lock().map_err(|e| e.to_string())?;
    d.playing = false;
    Ok(())
}

pub mod commands;
pub mod engine;
pub mod events;
pub mod playlist;

#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::add_to_playlist,
            commands::play_index,
            commands::next,
            commands::prev,
            commands::pause,
            commands::seek,
        ])
        .manage(engine::engine::AudioEngine::new())
        .manage(std::sync::Mutex::new(playlist::model::Playlist::new()))
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

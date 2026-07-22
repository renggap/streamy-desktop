pub mod commands;
pub mod encode;
pub mod engine;
pub mod events;
pub mod playlist;
pub mod profile;
pub mod stream;

use tauri::Manager;

use profile::store::ProfileStore;

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
            commands::list_playlist,
            commands::play_index,
            commands::play,
            commands::next,
            commands::prev,
            commands::pause,
            commands::seek,
            commands::set_source_mode,
            commands::list_input_devices,
            commands::list_output_devices,
            commands::set_input_device,
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::connect_stream,
            commands::disconnect_stream,
            commands::set_stream_title,
        ])
        .manage(engine::engine::AudioEngine::new())
        .manage(std::sync::Mutex::new(
            None::<stream::broadcast::BroadcastHandle>,
        ))
        .manage(std::sync::Mutex::new(playlist::model::Playlist::new()))
        .setup(|app| {
            // Hand the audio thread an AppHandle so it can emit `error` events.
            app.state::<engine::engine::AudioEngine>()
                .set_app(app.handle().clone());
            // Connection-profile store in the OS app-data dir.
            app.manage(ProfileStore::new(app.path().app_data_dir()?));
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

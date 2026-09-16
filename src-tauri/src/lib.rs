pub mod commands;
pub mod diag;
pub mod encode;
pub mod engine;
pub mod events;
pub mod playlist;
pub mod profile;
pub mod record;
pub mod settings;
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
            commands::set_crossfade,
            commands::set_eq,
            commands::set_duck,
            commands::set_mic_denoise,
            commands::list_input_devices,
            commands::list_output_devices,
            commands::set_input_device,
            commands::set_output_device,
            commands::set_recording_dir,
            commands::set_volume,
            commands::set_monitor_file_volume,
            commands::get_settings,
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::connect_stream,
            commands::disconnect_stream,
            commands::set_stream_title,
            commands::start_recording,
            commands::stop_recording,
            commands::repair_webview2,
            commands::webview2_installed,
        ])
        .manage(engine::engine::AudioEngine::new())
        .manage(std::sync::Mutex::new(commands::BcastState::Idle))
        .manage(std::sync::Mutex::new(commands::RecState::Idle))
        .setup(|app| {
            // Diagnostics log + devtools (debug only).
            let data_dir = app.path().app_data_dir()?;
            let _ = std::fs::create_dir_all(&data_dir);
            diag::init(data_dir.join("streamy.log"));
            // Playlist lives in the OS app-data dir so tracks survive restarts.
            app.manage(std::sync::Mutex::new(playlist::model::Playlist::load_from(
                &data_dir,
            )));
            #[cfg(debug_assertions)]
            {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    w.open_devtools();
                }
            }

            // Hand the audio thread an AppHandle so it can emit `error` events.
            app.state::<engine::engine::AudioEngine>()
                .set_app(app.handle().clone());
            // Connection-profile + settings stores in the OS app-data dir.
            app.manage(ProfileStore::new_with_keyring(data_dir.clone()));
            app.manage(settings::SettingsStore::new(data_dir.clone()));

            // Restore the last-used mic input device.
            if let Some(name) = app.state::<settings::SettingsStore>().get().input_device {
                if let Err(e) = app
                    .state::<engine::engine::AudioEngine>()
                    .set_input_device(name.clone())
                {
                    diag::log(format!("restore input device '{name}' failed: {e}"));
                } else {
                    diag::log(format!("restored input device '{name}'"));
                }
            }
            // Restore the last-used output device.
            if let Some(name) = app.state::<settings::SettingsStore>().get().output_device {
                let _ = app
                    .state::<engine::engine::AudioEngine>()
                    .set_output_device(Some(name));
            }
            // Restore the master volume (defaults to unity / 100%).
            let vol = app.state::<settings::SettingsStore>().get().volume;
            let _ = app.state::<engine::engine::AudioEngine>().set_volume(vol);
            // Restore mic noise suppression (RNNoise, broadcast-only path).
            if app.state::<settings::SettingsStore>().get().mic_denoise {
                let _ = app.state::<engine::engine::AudioEngine>().set_denoise(true);
            }
            // Default launch width = 60% of the primary monitor (2x the old
            // 30%; user request). Logical units so HiDPI stays proportional.
            {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    if let Ok(Some(monitor)) = w.current_monitor() {
                        let size = monitor.size();
                        let scale = w.scale_factor().unwrap_or(1.0);
                        let logical_w = size.width as f64 / scale * 0.60;
                        if let Ok(cur) = w.inner_size() {
                            let logical_h = cur.to_logical::<f64>(scale).height;
                            let _ = w.set_size(tauri::LogicalSize::new(logical_w, logical_h));
                        }
                    }
                }
            }
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

// Release builds are GUI-only; without this Windows opens a console window
// behind the launcher.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod content;
mod pack;
mod primary_action;
mod settings;
mod state;

use std::sync::Arc;

use tauri::Manager as _;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CAGALINTRY_LOG")
                .unwrap_or_else(|_| "info,cagalintry=debug".into()),
        )
        .init();

    tauri::Builder::default()
        // Two launchers sharing one pack directory would race on the same
        // files, so a second launch focuses the existing window instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::list_packs,
            commands::get_pack,
            commands::list_minecraft_versions,
            commands::list_loader_versions,
            commands::create_pack,
            commands::update_pack,
            commands::delete_pack,
            commands::open_pack_folder,
            commands::launch_pack,
            commands::kill_pack,
            commands::search_content,
            commands::get_project,
            commands::list_project_versions,
            commands::list_content,
            commands::install_content,
            commands::remove_content,
            commands::set_content_enabled,
            commands::get_settings,
            commands::update_settings,
            commands::data_directory,
        ])
        .setup(|app| {
            // Behind an Arc so background tasks — the process watcher, the
            // progress aggregator — can hold it past the borrow of a command.
            let state = Arc::new(tauri::async_runtime::block_on(state::AppState::new())?);
            tracing::info!(root = %state.dirs.root().display(), "data directory");
            app.manage(state);

            // The window starts hidden so the first paint is the finished UI
            // rather than a white rectangle.
            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Cagalintry Launcher");
}

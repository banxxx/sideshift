// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod core;
mod models;
mod task_engine;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = task_engine::AppState::new(app.handle());
            app.manage(std::sync::Arc::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::parse_pack,
            commands::list_mc_versions,
            commands::list_loader_versions,
            commands::list_java_versions,
            commands::default_options,
            commands::list_pack_dirs,
            commands::get_plan,
            commands::classify_pack,
            commands::inspect_added_mod,
            commands::list_excluded_mods,
            commands::search_mods,
            commands::list_mod_versions,
            commands::list_mod_categories,
            commands::estimate_download,
            commands::start_conversion,
            commands::list_tasks,
            commands::get_task,
            commands::cancel_task,
            commands::retry_task,
            commands::delete_task,
            commands::get_report,
            commands::get_settings,
            commands::set_settings,
            commands::list_download_sources,
            commands::check_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

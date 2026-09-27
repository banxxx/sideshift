// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
mod core;
mod l10n;
mod models;
mod task_engine;

use tauri::{Manager, WebviewWindowBuilder};

/// 主窗口由这里建，而不是交给 Tauri 自动建（配置里那枚 `"create": false` 就是这件事的开关）。
/// 唯一的原因：WebView2 的数据目录只认**建窗时**给的绝对路径，而我们要把它落在配置目录里——
/// 配置目录跟着安装目录走，卸载才带得动那几百 MB（本机实测 570MB，其中 532MB 是一次性缓存）。
/// 仍然走 `from_config` ⇒ 尺寸/透明/无边框这些选项的单源还是 tauri.conf.json，这里不抄第二份
fn build_main_window(app: &mut tauri::App) -> tauri::Result<()> {
    let Some(cfg) = app.config().app.windows.first().cloned() else {
        return Ok(());
    };
    let mut window = WebviewWindowBuilder::from_config(app.handle(), &cfg)?;
    if let Some(dir) = task_engine::webview_profile_dir(app.handle()) {
        window = window.data_directory(dir);
    }
    window.build().map(|_| ())
}

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
            build_main_window(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::parse_pack,
            commands::ensure_parsed,
            commands::list_mc_versions,
            commands::list_loader_versions,
            commands::probe_java,
            commands::java_requirement,
            commands::default_options,
            commands::list_pack_dirs,
            commands::get_plan,
            commands::classify_pack,
            commands::inspect_added_mod,
            commands::inspect_added_build,
            commands::list_excluded_mods,
            commands::search_mods,
            commands::list_mod_versions,
            commands::list_mod_categories,
            commands::mod_translate_zh,
            commands::estimate_download,
            commands::start_conversion,
            commands::list_tasks,
            commands::get_task,
            commands::cancel_task,
            commands::retry_task,
            commands::delete_task,
            commands::list_trash,
            commands::restore_task,
            commands::clear_trash,
            commands::get_report,
            commands::get_task_plan,
            commands::get_settings,
            commands::set_settings,
            commands::cache_usage,
            commands::clean_junk,
            commands::clean_cache,
            commands::list_download_sources,
            commands::check_update,
            commands::ack_snapshot,
            commands::ack_refresh,
            commands::ack_skins,
            commands::ack_skin_texture_get,
            commands::ack_skin_texture_put,
            commands::open_local_path,
            commands::reveal_local_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

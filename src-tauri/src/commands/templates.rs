use super::*;

/* ---------------- 转换模板 ---------------- */

/// 整张模板表（数组顺序 = 转换页那颗下拉的顺序，前端拖动排序后整表写回）
#[tauri::command]
pub fn list_templates(state: S<'_>) -> Vec<ConversionTemplate> {
    lock(&state).templates.clone()
}

/// 整表写回。改动模板只有「列表页拖完/删完」和「编辑页保存」两个出口，两处都交回全量 ⇒
/// 后端不必知道哪几条变了，也不会出现排序落了一半的中间态。
#[tauri::command]
pub fn set_templates(
    app: AppHandle,
    state: S<'_>,
    templates: Vec<ConversionTemplate>,
) -> Result<(), String> {
    // 先落盘再改内存（与 set_settings 同一条规矩）：写失败时内存里还是旧那张表，前端据此回滚
    task_engine::save_templates(&app, &templates)?;
    lock(&state).templates = templates;
    Ok(())
}

/// 新建模板的种子：模板收的那 16 档全部给值，编辑页勾哪档才真的写进模板。
/// 初值取自 `ConversionOptions::default()`，只有 `installLoaderLocally` 跟全局设置走
/// （和 `default_options` 同源）——默认值在 Rust 只有这一份，前端抄字面量的话改默认就得出两处。
#[tauri::command]
pub fn template_defaults(state: S<'_>) -> TemplateValues {
    let options = ConversionOptions {
        install_loader_locally: lock(&state).settings.install_loader_locally,
        ..Default::default()
    };
    TemplateValues::seeded_from(&options)
}


use super::*;

/* ---------------- 系统集成 ---------------- */

/// 用系统默认程序打开目录/文件（列表卡「打开输出目录」、报告页「打开文件夹」）。
///
/// 为什么在后端开而不让 JS 调 `openPath`：插件的 JS 命令受 capability scope 约束，
/// 只能命中清单里预先声明的目录（`$HOME/**` 那类），而输出目录是用户在原生对话框里
/// 自选的，可能是任意盘任意路径，枚举不完；命中不了就报 `opener:006 Not allowed to open path`。
/// 后端调用与其余文件 IO 同属可信代码，且分隔符在这里统一成本机写法，前端不必再关心。
#[tauri::command]
pub async fn open_local_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(native_path(&path), None::<&str>)
        .map_err(|_| app_code("open"))
}

/// 在系统文件管理器中定位文件
#[tauri::command]
pub async fn reveal_local_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(native_path(&path))
        .map_err(|_| app_code("reveal"))
}


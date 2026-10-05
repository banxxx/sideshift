use super::*;

/* ---------------- 设置 / 元信息 ---------------- */

#[tauri::command]
pub fn get_settings(state: S<'_>) -> AppSettings {
    lock(&state).settings.clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: S<'_>, settings: AppSettings) -> Result<(), String> {
    // 手输/粘贴的目录可能带正斜杠，存下来一律先归成本机分隔符（否则 opener 打不开）
    let settings = settings.normalized();
    // 先落盘再改内存：写失败时内存仍是旧值，前端据此回滚，不会出现「界面已生效、重启又变回去」
    task_engine::save_settings(&app, &settings)?;
    // 换目录后给新那口缓存盖章（判据见 `core::data_root::claim_cache_root`：只有本次真的
    // 建出来的目录才算我们造的）。盖不上不影响能用，只影响卸载时敢不敢删它
    let _ =
        crate::core::data_root::claim_cache_root(&std::path::PathBuf::from(&settings.cache_dir));
    lock(&state).settings = settings;
    Ok(())
}


/// 下载源档位。**只列真实存在的两条**：原来的「GitHub Releases」既不是 Maven 镜像、
/// 也没有任何代码走它，留着等于给用户一个假选项（镜像覆盖边界见 `core::downloader::source`）。
#[tauri::command]
pub fn list_download_sources() -> Vec<VersionOption> {
    vec![
        VersionOption {
            value: "official".into(),
            label: "官方源".into(),
            recommended: Some(true),
            group: None,
        },
        VersionOption {
            value: "bmclapi".into(),
            label: "BMCLAPI 国内镜像".into(),
            recommended: Some(false),
            group: None,
        },
    ]
}

/* ---------------- 缓存占用与清理 ---------------- */

/// 缓存是否正在被用。运行中的流水线在往 `files` 写、往 `tasks` 暂存，排队的随时会被调度起来，
/// 两种都算忙——命令层据此挡掉「清空全部」。
fn cache_busy(inner: &task_engine::Inner) -> bool {
    inner
        .tasks
        .values()
        .any(|t| matches!(t.status, TaskStatus::Queued | TaskStatus::Running))
}

/// 从锁里只取出清理需要的三样事实（缓存目录 / 在册任务 id / 忙标志），扫描与删除全在锁外做：
/// 巨包的目录遍历是几秒级的 IO，握着全局锁不放就是「清理时整个应用点不动」。
fn cache_targets(
    inner: &task_engine::Inner,
) -> (PathBuf, std::collections::BTreeSet<String>, bool) {
    (
        PathBuf::from(&inner.settings.cache_dir),
        inner.tasks.keys().cloned().collect(),
        cache_busy(inner),
    )
}

#[tauri::command]
pub async fn cache_usage(state: S<'_>) -> Result<CacheUsage, String> {
    let (dir, ids, busy) = cache_targets(&lock(&state));
    tauri::async_runtime::spawn_blocking(move || cleanup::usage(&dir, &ids, busy))
        .await
        .map_err(|_| app_code("panic"))
}

/// 清理无用文件：半截下载 + 孤儿暂存目录 + 空壳目录。下载缓存本体一个字节都不碰，
/// 所以这条不挡运行中的任务（忙碌时扫描会自动跳过可能正在写的 `.part`）。
/// 目录归属闸（`cleanup::cache_dir_is_ours`）不过时返回那条拒绝话术
#[tauri::command]
pub async fn clean_junk(state: S<'_>) -> Result<CleanReport, String> {
    let (dir, ids, busy) = cache_targets(&lock(&state));
    tauri::async_runtime::spawn_blocking(move || cleanup::clean_junk(&dir, &ids, busy))
        .await
        .map_err(|_| app_code("panic"))?
}

/// 清理下载缓存。`mode` = `stale`（只删过期）或 `all`（清空）。
///
/// 为什么只有 `all` 挡运行中的任务：`stale` 的判据是「最后一次使用」，而缓存每次命中复用都会
/// 刷新 mtime（`downloader::util::mark_used`），刚被在用的文件必然是 now，删不到它。
/// `all` 没有这层保护，正被流水线取用的文件说删就删。
#[tauri::command]
pub async fn clean_cache(state: S<'_>, mode: String) -> Result<CleanReport, String> {
    let (dir, _, busy) = cache_targets(&lock(&state));
    let mode = match mode.as_str() {
        "stale" => cleanup::CleanMode::Stale,
        "all" if !busy => cleanup::CleanMode::All,
        "all" => {
            return Err("有任务正在转换或排队中，清空缓存会删掉它在用的文件".into());
        }
        other => return Err(format!("未知的清理口径：{other}")),
    };
    tauri::async_runtime::spawn_blocking(move || cleanup::clean_cache(&dir, mode))
        .await
        .map_err(|_| app_code("panic"))?
}

/// 清理 loader 复用安装（`installs\{loader}\{mc}-{ver}`）。
/// 与下载缓存分档的原因见 `cleanup::clean_installs`：它是复用资产不是垃圾。
/// 忙碌闸与「清空全部」同一道：安装中的 loader 目录此刻正在被写、被取用
#[tauri::command]
pub async fn clean_installs(state: S<'_>) -> Result<CleanReport, String> {
    let (dir, _, busy) = cache_targets(&lock(&state));
    if busy {
        return Err("有任务正在转换或排队中，它的 loader 安装可能正在被取用".into());
    }
    tauri::async_runtime::spawn_blocking(move || cleanup::clean_installs(&dir))
        .await
        .map_err(|_| app_code("panic"))?
}


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

/// 检查更新（Rust: check_update -> 该渠道的最新版本与是否有更新）。
///
/// 端点是 `/releases`（列表）而**不是** `/releases/latest`：官方定义 latest 只返回
/// "most recent non-prerelease, non-draft release"，所以 Beta 包在那条线上永远看不见自己该收的版本。
/// 渠道优先用用户在设置页选的；没选过时看这一枚包自己的版本号带不带预发布位——
/// 新装用户一个设置都没动也不会站错队。
/// 比较走 semver：`1.0.0-beta.2` 比 `1.0.0-beta.10` 新，字符串比较正好比反。
#[tauri::command]
pub async fn check_update(app: AppHandle, state: S<'_>) -> Result<UpdateInfo, String> {
    let picked = lock(&state).settings.update_channel;
    let current = app.package_info().version.to_string();
    let cur = semver::Version::parse(current.trim_start_matches('v'))
        .map_err(|e| format!("本地版本号不是合法 semver（{current}）：{e}"))?;
    let want_prerelease = picked.map(|c| c == UpdateChannel::Beta).unwrap_or(!cur.pre.is_empty());

    let dl = downloader_of(&state);
    const UPDATE_URL: &str = "https://api.github.com/repos/banxxx/sideshift/releases?per_page=30";
    let v = dl
        .client
        .get(UPDATE_URL)
        .send()
        .await
        .map_err(|e| reqwest_code(&e, UPDATE_URL))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| reqwest_code(&e, UPDATE_URL))?;
    // 仓库还没有任何 release 时 GitHub 回的是 `{"message": "Not Found"}` 对象，不是数组。
    // 失败这件事要原样递到界面上（不能装作"已是最新版本"），但它那句英文不用：按同一口径归类，
    // 前端出「GitHub 上没有找到对应内容」。限流单独归 `busy`——两者给用户的下一动作不一样
    let list = v.as_array().ok_or_else(|| {
        let msg = v["message"].as_str().unwrap_or("");
        net_code(UPDATE_URL, if msg.contains("rate limit") { 429 } else { 404 })
    })?;

    let mut best: Option<semver::Version> = None;
    for r in list {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        // 渠道对不上的一律跳过：正式版用户不该被推测试包，反之亦然
        if r["prerelease"].as_bool().unwrap_or(false) != want_prerelease {
            continue;
        }
        let Some(tag) = r["tag_name"].as_str() else { continue };
        let Ok(ver) = semver::Version::parse(tag.trim_start_matches('v')) else {
            continue;
        };
        if best.as_ref().map_or(true, |b| &ver > b) {
            best = Some(ver);
        }
    }

    Ok(UpdateInfo {
        has_update: best.as_ref().is_some_and(|b| b > &cur),
        latest: best.map(|b| b.to_string()),
        current,
    })
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
        .map_err(|e| e.to_string())
}

/// 清理无用文件：半截下载 + 孤儿暂存目录 + 空壳目录。下载缓存本体一个字节都不碰，
/// 所以这条不挡运行中的任务（忙碌时扫描会自动跳过可能正在写的 `.part`）。
#[tauri::command]
pub async fn clean_junk(state: S<'_>) -> Result<CleanReport, String> {
    let (dir, ids, busy) = cache_targets(&lock(&state));
    tauri::async_runtime::spawn_blocking(move || cleanup::clean_junk(&dir, &ids, busy))
        .await
        .map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())
}


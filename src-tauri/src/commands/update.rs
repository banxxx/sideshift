//! 应用更新（设置 · 外观与关于）。检测、取件与装的本事都在 `core::update`，这里只管取本地事实、
//! 上那道只有拿得到全局状态的层才上的闸门，以及组装返回值。
//!
//! 检查只有 `gather` 一个实现点：设置里那颗手动钮与启动后那一趟自动检查走同一段代码。
//! 「设置页说有新版、角标说没有」这种两边各自自洽的错，在这里没有第二条路径能长出来。

use std::time::Duration;

use tauri::Manager;

use super::*;
use crate::core::{data_root, update};

/// 这一轮的本地事实。`check_update` 与 `prepare_update` 共用同一份取法，
/// 否则会出现「界面按 Beta 推包、下载按正式版验包」这种两边各自自洽的错。
struct Local {
    cache_dir: PathBuf,
    channel: UpdateChannel,
    /// 本机版本串（原样，界面显示用）
    current: String,
    /// 它的 semver 解析结果（比较用）
    cur: semver::Version,
}

fn local_facts(app: &AppHandle, state: &Arc<AppState>) -> Result<Local, String> {
    let current = app.package_info().version.to_string();
    let cur = semver::Version::parse(current.trim_start_matches('v'))
        .map_err(|e| format!("本地版本号不是合法 semver（{current}）：{e}"))?;
    // 渠道优先用用户在设置页选的；没选过时看这一枚包自己的版本号带不带预发布位——
    // 新装用户一个 setting 都没动也不会站错队
    let channel = update::channel_for(lock(state).settings.update_channel, &cur);
    let cache_dir = PathBuf::from(&lock(state).settings.cache_dir);
    Ok(Local {
        cache_dir,
        channel,
        current,
        cur,
    })
}

/// 敲那一趟，把版本比较、渠道、能不能一键装全部算完再回。
///
/// 端点是 release **列表**而不是 `/releases/latest`（原因见 `core::update::releases_url`）；
/// 判据都在 Rust 侧算，前端只渲染。取 JSON 走 `update::api_json`（10s 短超时 + 两次尝试，
/// 全局 120s 在这一档是黑洞；API 层无镜像，原因见那边的注释）。
async fn gather(app: &AppHandle, state: &Arc<AppState>) -> Result<UpdateInfo, String> {
    let Local {
        channel,
        current,
        cur,
        ..
    } = local_facts(app, state)?;

    let dl = downloader_of(state);
    let url = update::releases_url();
    let body = update::api_json(&dl.client, &url).await?;
    let releases = update::release::parse(&body, &url)?;

    // 便携形态（exe 旁边就是 portable.flag）一期不给一键更新：
    // 换掉正在运行的裸 exe 要另写一套自替换流程，不是加个分支的事
    let portable = data_root::portable_root().is_some();
    let best = update::release::latest_in_channel(&releases, channel);

    Ok(update::release::build_info(
        current,
        &cur,
        channel,
        best,
        portable,
        update::updater_pubkey().is_some(),
    ))
}

/// 检查更新（Rust: check_update -> 该渠道的最新版本与能不能应用内更新）。手动那颗钮。
///
/// 不读那道 24 小时闸门——闸门是给「用户没开口」的那些趟设的，人刚按了钮就该真敲一次。
#[tauri::command]
pub async fn check_update(app: AppHandle, state: S<'_>) -> Result<UpdateInfo, String> {
    let state = state.inner().clone();
    let info = gather(&app, &state).await?;
    // 落同一本账：明天那趟看见「今天才敲过」就不必再敲。`seen_ms` 也一并记成这一刻——
    // 结论此刻就摊在设置页上，人正站在那儿，没有比这更硬的「看过」
    if let Some(dir) = task_engine::config_dir(&app) {
        let now = chrono::Utc::now().timestamp_millis();
        let mut stamp = update::check::read(&dir);
        stamp.at_ms = now;
        stamp.seen_ms = now;
        stamp.info = Some(info.clone());
        update::check::write(&dir, &stamp);
    }
    Ok(info)
}

/// 冷启动那颗角标的初始值（Rust: update_badge -> 该亮就给那一版的结论，否则 null）。
///
/// 纯读那本账、不敲网络：角标要在界面画出来的那一刻就有答案，等一次网络等于先闪一个空位
#[tauri::command]
pub fn update_badge(app: AppHandle) -> Option<UpdateInfo> {
    let dir = task_engine::config_dir(&app)?;
    let stamp = update::check::read(&dir);
    if stamp.badge() {
        stamp.info
    } else {
        None
    }
}

/// 记一次「这扇窗被打开过」（Rust: mark_update_seen）⇒ 角标灭，直到下一趟敲出新版本。
///
/// 同步、无返回值：这本账坏了最贵的结果只是角标多亮一次，不值得为它给前端加一条错误分支
#[tauri::command]
pub fn mark_update_seen(app: AppHandle) {
    let Some(dir) = task_engine::config_dir(&app) else {
        return;
    };
    let mut stamp = update::check::read(&dir);
    stamp.seen_ms = chrono::Utc::now().timestamp_millis();
    update::check::write(&dir, &stamp);
}

/// 启动后那一趟自动检查。由 `lib.rs` 的 setup spawn，前端不感知它的存在，只收得着事件。
///
/// 两道「不打扰」：满 24 小时才真敲（账本管频次，dev 与 release 同一条路、同一本账——
/// 那道「dev 一趟不发」的闸撤了：有账本在，开发机一天重启几十次也只是一次真网络，
/// 而留着它 dev 里整条角标链就没法测）；失败完全不吭声，界面上不留一句。查出可装的版本只发
/// `update://available` 让角标亮，**不弹窗**——用户在忙别的时跳出一扇升级窗是打断。
pub(crate) async fn startup_check(app: AppHandle) {
    // 让开启动那几秒：splash、语言目录、占用扫描都在抢同一段主线程和同一份磁盘
    tokio::time::sleep(Duration::from_millis(8_000)).await;

    let Some(dir) = task_engine::config_dir(&app) else {
        return;
    };
    let mut stamp = update::check::read(&dir);
    let now = chrono::Utc::now().timestamp_millis();
    if !stamp.due(now) {
        return;
    }
    // 这里只有克隆出来的 Arc，没有 `State` 句柄（那玩意儿跨不了 `await`）
    let Some(state) = app
        .try_state::<Arc<AppState>>()
        .map(|s| s.inner().clone())
    else {
        return;
    };

    // 敲失败就沿用上一轮的结论：昨天查到的新版本今天仍然值得提醒，而「这趟没网」不该把角标抹掉。
    // 代价说清楚：那条 release 可能已被撤下，而取件那一轮自己会再查一遍
    stamp.info = match gather(&app, &state).await {
        Ok(info) => Some(info),
        Err(_) => stamp.info.clone(),
    };
    stamp.at_ms = now;
    update::check::write(&dir, &stamp);

    if stamp.badge() {
        if let Some(info) = &stamp.info {
            let _ = app.emit(update::check::EVENT_AVAILABLE, info);
        }
    }
}

/// 「跳过这个版本」（Rust: skip_update）⇒ 角标灭，且渠道里不出现**更新**的那一条之前不再亮
/// （判定在 `check::badge`，按 semver 比，被跳过的那版被撤回也不会拿更老的顶上来）。
/// 版本由前端从弹窗递回来（它正显示着那一版），归一掉 `v` 前缀再进账。
/// 同步、无返回值，与 `mark_update_seen` 同一口径：这本账坏了最贵的结果只是多提醒一次
#[tauri::command]
pub fn skip_update(app: AppHandle, version: String) {
    let Some(dir) = task_engine::config_dir(&app) else {
        return;
    };
    let version = version.trim().trim_start_matches('v').to_string();
    if version.is_empty() {
        return;
    }
    let mut stamp = update::check::read(&dir);
    stamp.skipped = Some(version);
    stamp.seen_ms = chrono::Utc::now().timestamp_millis();
    update::check::write(&dir, &stamp);
}

/// 取件（Rust: prepare_update -> 这一版的下载与验签结果）。进度另走 `update://progress`。
///
/// 只有一个入口，且**它自己会再检查一遍**能不能装（没内置公钥、跨渠道、降级、缺件都不放行）：
/// 界面上那颗按钮亮起的那一刻早已过去，中间 release 可能被人编辑过。
#[tauri::command]
pub async fn prepare_update(
    app: AppHandle,
    state: S<'_>,
    tag: String,
) -> Result<UpdateStatus, String> {
    let l = local_facts(&app, &state)?;
    update::fetch::prepare(&app, &l.cache_dir, l.channel, &l.current, &tag).await
}

/// 取消这一轮：正在下就立旗（半截由那一轮自己收掉），已经定稿就把暂存收走。
///
/// 同步而不是 async：它总是成功，返回 `Result` 只是逼前端多写一条永远不会走的错误分支。
/// 盘的那一小段（删一个最多两三个文件的目录）不值得起 `spawn_blocking`。
#[tauri::command]
pub fn cancel_update(state: S<'_>) -> UpdateStatus {
    let cache_dir = PathBuf::from(&lock(&state).settings.cache_dir);
    update::fetch::cancel(&cache_dir)
}

/// 当前这一档（弹窗重开、或冷启动时问出「上一轮办到哪一步」）。纯读内存，不敲网络。
#[tauri::command]
pub fn update_status() -> UpdateStatus {
    update::fetch::status()
}

/// 装（Rust: install_update）：静默跑官方安装器换回原目录，然后退出本进程。
///
/// 命令层只补「现在能不能装」这两问（有没有转换在跑、配置目录拿不拿得到）；
/// 「怎么装」与它那些承重约束（`/UPDATE`、账本先落盘）全在 `core::update::install` 一处。
/// 成功那一路不会回到调用方：这个进程马上就要没了，所以前端只该准备失败那一句。
#[tauri::command]
pub fn install_update(app: AppHandle, state: S<'_>) -> Result<(), String> {
    // 硬退会把跑一半的任务存档停在「运行中」，下次启动它就是一条永远不动的任务。不给强制档：
    // 等一条转换跑完是几十秒，而设置被清掉是回不来的那种
    if task_engine::has_active_tasks(&state) {
        return Err(app_code("update-tasks-busy"));
    }
    let current = app.package_info().version.to_string();
    let config_dir = task_engine::config_dir(&app).ok_or_else(|| app_code("update-config-dir"))?;
    update::install::install(&app, &config_dir, &current)
}

/// 上一次「立即安装」的结论（Rust: update_outcome）。账本读一次即收走 ⇒ 第二次调它是 null。
///
/// 顺带把装成功那一版的暂存收掉：那两个字节此刻已经变成装进机器里的程序，
/// 留在缓存里只是白占几 MB；没装成的那一对**留着**——重下一轮靠它直接跳过下载。
#[tauri::command]
pub fn update_outcome(app: AppHandle, state: S<'_>) -> Option<UpdateOutcome> {
    let current = app.package_info().version.to_string();
    let config_dir = task_engine::config_dir(&app)?;
    let outcome = update::install::take_outcome(&config_dir, &current)?;
    if outcome.kind == UpdateOutcomeKind::Done {
        let cache_dir = PathBuf::from(&lock(&state).settings.cache_dir);
        update::fetch::discard(&cache_dir, &outcome.attempted);
    }
    Some(outcome)
}

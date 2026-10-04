//! 应用更新（设置 · 外观与关于）。检测、取件与装的本事都在 `core::update`，这里只管取本地事实、
//! 上那道只有拿得到全局状态的层才上的闸门，以及组装返回值。

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

fn local_facts(app: &AppHandle, state: &S<'_>) -> Result<Local, String> {
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

/// 检查更新（Rust: check_update -> 该渠道的最新版本与能不能应用内更新）。
///
/// 端点是 release **列表**而不是 `/releases/latest`（原因见 `core::update::releases_url`）；
/// 渠道与比较都在 Rust 侧算完，前端只渲染，判据只有一个实现点。
#[tauri::command]
pub async fn check_update(app: AppHandle, state: S<'_>) -> Result<UpdateInfo, String> {
    let Local {
        channel,
        current,
        cur,
        ..
    } = local_facts(&app, &state)?;

    let dl = downloader_of(&state);
    let url = update::releases_url();
    let body = dl
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| reqwest_code(&e, &url))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| reqwest_code(&e, &url))?;
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

/// 上一次「重启并安装」的结论（Rust: update_outcome）。账本读一次即收走 ⇒ 第二次调它是 null。
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

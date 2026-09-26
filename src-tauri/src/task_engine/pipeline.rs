//! 流水线：parser ≤15 · detector ≤30 ·（可选）本机装加载器 ≤42 · 取件 ≤82 · builder ≤100。
//! 阶段进度口径与前端 mock 引擎一致。本机安装排在模组取件**之前**（跑不成越早停越好），
//! 它从下载档头部切走 30→42 这一格；开关关着时主轮仍从 30 起，整条链路的写值逐字节相同。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::AppHandle;

use crate::core::builder::{self, BuildEvent, BuildInput, BuilderError};
use crate::core::detector;
use crate::core::downloader::{
    DownloadError, Downloader, Fetch, FetchSource, ItemSpec, TransferProgress,
};
use crate::core::installer::{self, InstallEvent, Installed};
use crate::core::java;
use crate::core::parser::{self, ParsedPack};
use crate::core::verify;
use crate::models::*;
use super::activity::{
    apply_fetch_counts, build_progress, fetch_group_label, flush_groups, group_line,
    group_plan_of, install_progress, reports_per_line, FetchGroups, NetActivity, ZipActivity,
    FETCH_FROM, INSTALL_FROM, INSTALL_TO, LOADER_JAR_TO,
};
use super::events::{
    emit_progress, fail, log_line, log_update, notify_done, push_line, push_log, update,
};
use super::persist::save_tasks;
use super::schedule::{release_and_next, remove_task_staging};
use super::state::{is_active, AppState};
use super::util::{brief_list, fmt_size, now_ms, sanitize, unique_mod_name};
use super::CACHE_TASKS_DIR;

/// 在后台跑一条流水线；退出后回收暂存并拉起队首。
/// （原「独占槽位交接点」注释：流水线退出——成功/失败/取消皆如此——后交回调度器）
pub fn spawn_pipeline(app: &AppHandle, state: &Arc<AppState>, id: String) {
    let (a, s) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        run_pipeline(a.clone(), s.clone(), id.clone()).await;
        // 失败与取消不走成功收尾，暂存目录统一在这里回收
        remove_task_staging(&s, &id);
        if let Some(next) = release_and_next(&a, &s, &id) {
            spawn_pipeline(&a, &s, next);
        }
    });
}

async fn run_pipeline(app: AppHandle, state: Arc<AppState>, id: String) {
    let (plan, pack, options) = {
        let inner = state.inner.lock().unwrap();
        let task = match inner.tasks.get(&id) {
            Some(t) => t,
            None => return,
        };
        // 排队期间被取消（或已删除）：不进入运行态，交回调度器拉起下一队
        if !matches!(task.status, TaskStatus::Queued | TaskStatus::Running) {
            return;
        }
        (
            inner.plans.get(&id).cloned().unwrap_or_default(),
            task.pack.clone(),
            task.options.clone(),
        )
    };

    /* ---- 阶段 1 · parser ---- */
    update(&app, &state, &id, |t| {
        t.status = TaskStatus::Running;
        t.stage = Some(PipelineStage::Parser);
        t.started_at = Some(now_ms());
        t.progress = 8;
    }, true);
    let parsed: Arc<ParsedPack> = {
        let cached = state.inner.lock().unwrap().parsed_by_name.get(&pack.file_name).cloned();
        match cached {
            Some(p) => p,
            None => {
                let path = pack
                    .source_path
                    .as_deref()
                    .map(PathBuf::from)
                    .filter(|p| p.exists());
                match path {
                    Some(p) => Arc::new(parser::parse(&p)),
                    None => {
                        fail(&app, &state, &id, TaskError {
                            stage: PipelineStage::Parser,
                            title: "解析失败".into(),
                            detail: "找不到源整合包文件，请返回首页重新选择".into(),
                            code: None,
                            retryable: false,
                            attempts: None,
                            log_tail: None,
                            exit_code: None,
                        });
                        return;
                    }
                }
            }
        }
    };
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Parser,
        LogLevel::Info,
        &format!(
            "读取清单 {} · minecraft-{}",
            pack.file_name, parsed.manifest.mc_version
        ),
    );
    // index 声明了 URL 但包内没字节的残缺条目才需要联网补取
    let off_pack = parsed
        .mod_files
        .iter()
        .chain(parsed.extra_files.iter())
        .filter(|f| !f.in_pack && !f.url.is_empty())
        .count();
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Parser,
        LogLevel::Info,
        &format!(
            "包内内容：模组 {} 个 · 其他文件 {} 个 · 需联网补取 {} 个",
            parsed.mod_files.len(),
            parsed.extra_files.len(),
            off_pack
        ),
    );
    update(&app, &state, &id, |t| t.progress = 15, false);
    if !is_active(&state, &id) {
        return;
    }

    /* ---- 阶段 2 · detector ---- */
    update(&app, &state, &id, |t| t.stage = Some(PipelineStage::Detector), false);
    let counts = detector::count_plan(&plan);
    let review: Vec<String> = plan
        .iter()
        .filter(|m| m.needs_review)
        .map(|m| m.name.clone())
        .collect();
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Detector,
        LogLevel::Info,
        &format!("方案确认：剔除 {} · 保留 {} · 新增 {}", counts.remove, counts.keep, counts.add),
    );
    let removed: Vec<String> = plan
        .iter()
        .filter(|m| m.disposition == ModDisposition::Remove)
        .map(|m| m.name.clone())
        .collect();
    if !removed.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Detector,
            LogLevel::Info,
            &format!("剔除名单：{}", brief_list(&removed)),
        );
    }
    for row in plan
        .iter()
        .filter(|m| m.auto_supplement && m.disposition != ModDisposition::Remove)
    {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Detector,
            LogLevel::Info,
            &format!("自动补齐：{}（服务端运行所需）", row.name),
        );
    }
    if !review.is_empty() {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Detector,
            LogLevel::Warn,
            &format!("待人工确认：{}（跨版本组件，服务端可能仍需）", brief_list(&review)),
        );
    }
    update(
        &app,
        &state,
        &id,
        |t| {
            t.counts = Some(counts);
            t.progress = 30;
        },
        true,
    );
    if !is_active(&state, &id) {
        return;
    }

    /* ---- 阶段 3 · downloader ---- */
    update(&app, &state, &id, |t| {
        t.stage = Some(PipelineStage::Downloader);
        t.progress = 31;
    }, true);
    let settings = state.inner.lock().unwrap().settings.clone();
    let staging = PathBuf::from(&settings.cache_dir)
        .join(CACHE_TASKS_DIR)
        .join(&id)
        .join("staging");
    let _ = std::fs::remove_dir_all(&staging);
    let mods_dir = staging.join("mods");
    let mut dl = Downloader::new(
        PathBuf::from(&settings.cache_dir),
        settings.concurrency as usize,
    )
    .with_source(settings.download_source.normalized())
    .with_curseforge_key(settings.curseforge_api_key.clone());
    let source_path = parsed
        .manifest
        .source_path
        .clone()
        .map(PathBuf::from)
        .unwrap_or_default();

    let mut items: Vec<ItemSpec> = Vec::new();
    let mut used_files: HashSet<usize> = HashSet::new();
    // mods/ 落位文件名（大小写口径）：防不同目录同名 jar 静默互相覆盖
    let mut used_names: HashSet<String> = HashSet::new();
    let mut server_jar_name: Option<String> = None;
    let mut installer_jar_name: Option<String> = None;
    // 开了本机安装时，官方安装器 jar 被提前到「阶段 2.5」单独取，不进主轮计划：
    // 跑不成要越早停越好，不能让用户等完几 GB 模组才发现任务要重跑
    let mut loader_first: Option<ItemSpec> = None;

    // 3.1 保留 + 新增的模组
    for row in plan.iter().filter(|m| m.disposition != ModDisposition::Remove) {
        // 在线添加的钉住行最先匹配：用户选哪个构建，构建时就下哪个（不再解析最新版）
        if let Some(p) = &row.pinned {
            let file_name = unique_mod_name(&mut used_names, &p.file_name, &row.id);
            // CurseForge 的直链是带时效的签名 URL，建档时存不下来 → 构建期现取一条。
            // 拿不到必须停下：空 URL 下载要么报错要么落一个废 jar 进服务端包
            let url = if p.needs_curseforge_link() {
                let file_id = p.file_id.clone().unwrap_or_default();
                match dl.curseforge_download_url(&row.id, &file_id).await {
                    Ok(u) => u,
                    Err(e) => {
                        let detail = e.to_string();
                        fail(&app, &state, &id, TaskError {
                            stage: PipelineStage::Downloader,
                            title: "CurseForge 取链接失败".into(),
                            detail,
                            code: e.net_code(),
                            retryable: true,
                            attempts: None,
                            log_tail: None,
                            exit_code: None,
                        });
                        return;
                    }
                }
            } else {
                p.url.clone()
            };
            items.push(ItemSpec {
                fetch: Fetch::Url(url),
                sha1: p.sha1.clone(),
                dest: mods_dir.join(&file_name),
                file_name,
                size_bytes: row.size_bytes,
            });
            continue;
        }
        let matched = detector::match_pack_index(&parsed.mod_files, row, &used_files)
            .map(|i| (i, &parsed.mod_files[i]));
        if let Some((i, f)) = matched {
            used_files.insert(i);
            // 物理在包内一律 ZipEntry 直取（mrpack index 几乎总带 URL，不能以 URL 定夺）；
            // 仅「index 声明但包内缺字节」的残缺条目回落 URL 补下
            let fetch = if f.in_pack || f.url.is_empty() {
                Fetch::ZipEntry {
                    archive: source_path.clone(),
                    entry: f.path.clone(),
                }
            } else {
                Fetch::Url(f.url.clone())
            };
            let file_name = unique_mod_name(&mut used_names, &f.file_name, &row.id);
            items.push(ItemSpec {
                fetch,
                sha1: f.sha1.clone(),
                dest: mods_dir.join(&file_name),
                file_name,
                size_bytes: f.size_bytes,
            });
        } else if let Some(lp) = &row.local_path {
            // 本地 .jar：直接取本地文件
            let lp = PathBuf::from(lp);
            let name = lp
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("{}.jar", sanitize(&row.id)));
            let name = unique_mod_name(&mut used_names, &name, &row.id);
            let lp_size = std::fs::metadata(&lp).map(|m| m.len()).unwrap_or(0);
            items.push(ItemSpec {
                fetch: Fetch::Local(lp),
                file_name: name.clone(),
                sha1: None,
                dest: mods_dir.join(name),
                size_bytes: lp_size,
            });
        } else {
            // 外部新增：Modrinth 解析最新兼容构建
            match dl
                .resolve_mod_file(&row.id, &options.mc_version, parsed.manifest.loader)
                .await
            {
                Ok(mut spec) => {
                    spec.file_name = unique_mod_name(&mut used_names, &spec.file_name, &row.id);
                    spec.dest = mods_dir.join(&spec.file_name);
                    items.push(spec);
                }
                Err(e) => {
                    let detail = e.to_string();
                    fail(&app, &state, &id, TaskError {
                        stage: PipelineStage::Downloader,
                        title: "依赖解析失败".into(),
                        detail,
                        code: e.net_code(),
                        retryable: true,
                        attempts: None,
                        log_tail: None,
                        exit_code: None,
                    });
                    return;
                }
            }
        }
    }

    // 3.2 用户在「客户端保留目录」卡勾选的目录（逻辑相对路径，任意层级），命中前缀的文件原样带入；
    // overrides/ 壳前缀剥离后匹配与落位（CF 格式内容映射到服务端根）
    let mut kept_by_dir: HashMap<String, usize> = HashMap::new();
    for f in &parsed.extra_files {
        let rel = f.path.replace('\\', "/");
        let logical = parser::logical_rel(&rel);
        let lower = logical.to_lowercase();
        let hit = options
            .keep_dirs
            .iter()
            .find(|d| lower.starts_with(&format!("{}/", d.to_lowercase())));
        let Some(dir) = hit else { continue };
        *kept_by_dir.entry(dir.clone()).or_default() += 1;
        let dest = staging.join(logical);
        let fetch = if f.in_pack || f.url.is_empty() {
            Fetch::ZipEntry {
                archive: source_path.clone(),
                entry: f.path.clone(),
            }
        } else {
            Fetch::Url(f.url.clone())
        };
        items.push(ItemSpec {
            fetch,
            file_name: f.file_name.clone(),
            sha1: f.sha1.clone(),
            dest,
            size_bytes: f.size_bytes,
        });
    }
    for dir in &options.keep_dirs {
        let n = kept_by_dir.get(dir).copied().unwrap_or(0);
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            if n == 0 { LogLevel::Warn } else { LogLevel::Info },
            &format!(
                "保留目录 {dir} · {n} 个文件{}",
                if n == 0 { "（包内无此目录，已跳过）" } else { "" }
            ),
        );
    }

    // 3.3 服务端加载器本体（loader_version 为空会拼出无效坐标——前端已拦截，这里兜底）
    if options.loader_version.trim().is_empty() {
        fail(&app, &state, &id, TaskError {
            stage: PipelineStage::Downloader,
            title: "未选择加载器版本".into(),
            detail: "裸 zip 包无法自动确定 Loader 版本，请在转换配置中选择后重试".into(),
            code: None,
            retryable: false,
            attempts: None,
            log_tail: None,
            exit_code: None,
        });
        return;
    }
    match parsed.manifest.loader {
        LoaderKind::Fabric => {
            match dl.fabric_server_jar(&options.mc_version, &options.loader_version).await {
                Ok(mut spec) => {
                    server_jar_name = Some(spec.file_name.clone());
                    spec.dest = staging.join(&spec.file_name);
                    items.push(spec);
                }
                Err(e) => {
                    fail(&app, &state, &id, TaskError {
                        stage: PipelineStage::Downloader,
                        title: "服务端加载器获取失败".into(),
                        detail: e.to_string(),
                        code: e.net_code(),
                        retryable: true,
                        attempts: None,
                        log_tail: None,
                        exit_code: None,
                    });
                    return;
                }
            }
        }
        LoaderKind::Forge => {
            let mut spec = dl.forge_installer(&options.mc_version, &options.loader_version);
            // maven 伴生 .sha1：补上后走统一校验与 sha1 键缓存
            dl.attach_side_sha1(&mut spec).await;
            installer_jar_name = Some(spec.file_name.clone());
            spec.dest = staging.join(&spec.file_name);
            push_loader_item(options.install_loader_locally, &mut items, &mut loader_first, spec);
        }
        LoaderKind::NeoForge => {
            let mut spec = dl.neoforge_installer(&options.loader_version);
            dl.attach_side_sha1(&mut spec).await;
            installer_jar_name = Some(spec.file_name.clone());
            spec.dest = staging.join(&spec.file_name);
            push_loader_item(options.install_loader_locally, &mut items, &mut loader_first, spec);
        }
    }

    let loader_jar = server_jar_name.clone().or(installer_jar_name.clone());
    if let Some(jar) = &loader_jar {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            LogLevel::Info,
            &format!(
                "服务端加载器：{jar}（{} {}）",
                parsed.manifest.loader.as_label(),
                options.loader_version
            ),
        );
    }

    /* ---- 阶段 2.5 · 加载器就位（本机执行官方安装器，排在模组取件之前） ---- */
    // 排在这里的理由只有一条：本机装不出来是这条链路上最贵的失败（整条任务要重跑），
    // 放在几十 MB～几 GB 的模组下载之后，等于让用户等完才发现。
    // Fabric 走不到这里 —— 它没有安装器 jar 可提前（见 3.3 只在 Forge/NeoForge 分支落 loader_first）
    let cancel = state
        .inner
        .lock()
        .unwrap()
        .cancel
        .get(&id)
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    // 装好的 loader 树：打包阶段并进 staging（并进去之后 staging 就等于产物内容）
    let mut installed: Option<Installed> = None;
    // 主轮取件的起脚：没走这一档仍是 30→82，写值与没有安装档时逐字节相同
    let fetch_from = match loader_first {
        Some(spec) => {
            let jar = spec.dest.clone();
            if let Err(e) = prefetch_loader_jar(&app, &state, &id, &mut dl, &cancel, spec).await {
                map_download_error(&app, &state, &id, &e);
                return;
            }
            if !is_active(&state, &id) {
                push_log(
                    &app,
                    &state,
                    &id,
                    PipelineStage::Downloader,
                    LogLevel::Warn,
                    "任务已取消，取件中止",
                );
                return;
            }
            // 复用关时装进任务私有目录：remove_task_staging 收的是 cache/tasks/{id} 整个目录，
            // 「用完即弃」不用再另起一条回收路径
            let scratch = staging
                .parent()
                .map(|p| p.join("install-scratch"))
                .unwrap_or_else(|| staging.join("install-scratch"));
            match run_installer_stage(
                &app,
                &state,
                &id,
                InstallStage {
                    loader: parsed.manifest.loader,
                    mc_version: &options.mc_version,
                    loader_version: &options.loader_version,
                    java_required: &options.java_version,
                    java_selected: &options.java_path,
                    reuse: settings.reuse_loader_installs,
                    cache_dir: Path::new(&settings.cache_dir),
                    installer_jar: &jar,
                    scratch: &scratch,
                    cancel: &cancel,
                },
            )
            .await
            {
                Ok(v) => {
                    installed = Some(v);
                    INSTALL_TO
                }
                Err(InstallStop::Cancelled) => {
                    push_log(
                        &app,
                        &state,
                        &id,
                        PipelineStage::Installer,
                        LogLevel::Warn,
                        "任务已取消，本机安装已中止",
                    );
                    return;
                }
                Err(InstallStop::Failed(e)) => {
                    fail(&app, &state, &id, e);
                    return;
                }
            }
        }
        None => FETCH_FROM,
    };

    let total = items.len() as u32;
    // 自检要对账「计划落进 mods/ 的文件」，items 随后被 download_all 吃掉，此刻快照一次
    let expected_mod_files: Vec<String> = items
        .iter()
        .filter(|i| i.dest.starts_with(&mods_dir))
        .map(|i| i.file_name.clone())
        .collect();
    // 取件构成：只有「Fetch::Url 且未命中下载缓存」的条目真正走网络，
    // 包内条目与本地 jar 零流量 —— 界面据此不再把全程称作假下载的「下载中」
    let mut tally = FetchTally {
        files: total,
        ..Default::default()
    };
    // 缓存探测要重算 sha1（整文件读一遍），条目多时不能反复读盘：一次算完给统计和分组计划共用
    let cached_flags: Vec<bool> = items.iter().map(|it| dl.is_cached(it)).collect();
    for (it, cached) in items.iter().zip(&cached_flags) {
        tally.bytes += it.size_bytes;
        // 与 harvest 的分拣一致：命中缓存的条目走复制，不再解包
        if *cached {
            tally.cached_files += 1;
        } else {
            match &it.fetch {
                Fetch::Url(_) => {
                    tally.net_files += 1;
                    tally.net_bytes += it.size_bytes;
                }
                Fetch::ZipEntry { .. } => tally.pack_files += 1,
                Fetch::Local(_) => tally.local_files += 1,
            }
        }
    }
    // 离线条目的分目录账本计划（入组口径与取件回调同源）
    let group_plan = group_plan_of(&items, &staging, &cached_flags);
    update(&app, &state, &id, |t| {
        t.total = Some(total);
        t.downloaded = Some(0);
        t.fetch = Some(tally);
        t.net_done = Some(0);
        t.done_bytes = Some(0);
    }, false);
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Downloader,
        LogLevel::Info,
        &format!(
            "取件计划 {total} 项 · 需联网 {}（≈{}）· 整合包 {} · 本地 {} · 缓存命中 {} · 并发 {}",
            tally.net_files,
            fmt_size(tally.net_bytes),
            tally.pack_files,
            tally.local_files,
            tally.cached_files,
            settings.concurrency
        ),
    );
    if tally.net_files == 0 {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Downloader,
            LogLevel::Info,
            "本次无需联网：全部文件来自整合包、本地文件或下载缓存",
        );
    }

    let net_actual = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let bytes_actual = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (net_a, bytes_a) = (net_actual.clone(), bytes_actual.clone());
    let groups = Arc::new(Mutex::new(FetchGroups::new(group_plan)));
    /* 联网实时条：字节级回调比日志密两个数量级，只攒账、按窗口出一条 activity；
       日志仍然一个条目完成一行 */
    let agg = Arc::new(Mutex::new(NetActivity::new(tally.net_bytes, tally.net_files)));
    let (app_t, state_t, id_t) = (app.clone(), state.clone(), id.clone());
    let (agg_t, net_t) = (agg.clone(), net_actual.clone());
    let cancel_t = cancel.clone();
    let dl = dl.with_transfer(Arc::new(move |p: &TransferProgress| {
        if cancel_t.load(Ordering::Relaxed) {
            return;
        }
        // 锁顺序固定「先账本、后任务表」，任务表回调里不得回头锁账本
        let info = agg_t
            .lock()
            .unwrap()
            .record(p, net_t.load(Ordering::Relaxed));
        if let Some(info) = info {
            update(&app_t, &state_t, &id_t, |t| t.activity = Some(info), true);
        }
    }));
    let (app_f, state_f, id_f) = (app.clone(), state.clone(), id.clone());
    let (groups_f, net_f, bytes_f) = (groups.clone(), net_actual.clone(), bytes_actual.clone());
    let (agg_f, net_a2) = (agg.clone(), net_actual.clone());
    let staging_f = staging.clone();
    // 取消后 release_and_next 会把下一条排队任务标成 Running，届时 is_active(本 id)
    // 这类「有没有 Running 行」的判断会误判为真，故回调自己盯取消标志
    let cancel_flag = cancel.clone();
    let dl_result = dl
        .download_all(items, cancel.clone(), move |done, tot, oc| {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let network = matches!(oc.source, FetchSource::Network);
            net_a.fetch_add(if network && !oc.cached { 1 } else { 0 }, Ordering::Relaxed);
            bytes_a.fetch_add(oc.bytes, Ordering::Relaxed);

            // 真正联网（含重试）的条目值得逐条盯：量级只有几十到几百条
            if reports_per_line(network, oc.cached, oc.retries) {
                let verb = if oc.cached { "复用缓存" } else { "联网获取" };
                let mut msg = format!("{verb} {} · {}", oc.file_name, fmt_size(oc.bytes));
                if oc.retries > 0 {
                    msg.push_str(&format!("（重试 {} 次后成功）", oc.retries));
                }
                let level = if oc.retries > 0 { LogLevel::Warn } else { LogLevel::Info };
                let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                // 结算这条的在飞字节；没有下一条在飞就顺势收掉实时条
                let snap = {
                    let mut g = agg_f.lock().unwrap();
                    g.settle(&oc.dest, oc.bytes);
                    (!g.inflight.is_empty()).then(|| g.snapshot(net_a2.load(Ordering::Relaxed)))
                };
                log_update(
                    &app_f,
                    &state_f,
                    &id_f,
                    PipelineStage::Downloader,
                    level,
                    &msg,
                    move |t| {
                        apply_fetch_counts(t, done, tot, &net_u, &bytes_u, fetch_from);
                        t.activity = snap;
                    },
                );
                return;
            }

            // 离线项：记进所属目录的账，取满预设数才出一条；没取满时只按间隔推进度、不写日志
            let label = fetch_group_label(&oc.dest, &staging_f);
            let done_snap = groups_f.lock().unwrap().record(&label, oc.bytes, done, tot);
            match done_snap {
                Some(g) => {
                    let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                    log_update(
                        &app_f,
                        &state_f,
                        &id_f,
                        PipelineStage::Downloader,
                        LogLevel::Info,
                        &group_line(&g),
                        move |t| apply_fetch_counts(t, g.done, g.total, &net_u, &bytes_u, fetch_from),
                    );
                }
                None => {
                    if groups_f.lock().unwrap().beat_due() {
                        let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                        update(
                            &app_f,
                            &state_f,
                            &id_f,
                            |t| apply_fetch_counts(t, done, tot, &net_u, &bytes_u, fetch_from),
                            true,
                        );
                    }
                }
            }
        })
        .await;
    // 兜底只给「跑完了但账本没对上」的正常任务补行；取消/失败不补，
    // 否则会把半截目录说成取件完成，还会覆写已取消任务的进度
    if dl_result.is_ok() && is_active(&state, &id) {
        flush_groups(&app, &state, &id, &groups, &net_actual, &bytes_actual, fetch_from);
    }
    if let Err(e) = dl_result {
        map_download_error(&app, &state, &id, &e);
        return;
    }
    if !is_active(&state, &id) {
        push_log(&app, &state, &id, PipelineStage::Downloader, LogLevel::Warn, "任务已取消，取件中止");
        return;
    }
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Downloader,
        LogLevel::Info,
        &format!(
            "取件完成 {total} 项 · 实际联网 {} 项 · 共取件 {}",
            net_actual.load(Ordering::Relaxed),
            fmt_size(bytes_actual.load(Ordering::Relaxed))
        ),
    );
    update(&app, &state, &id, |t| t.progress = 82, true);

    /* ---- 阶段 4 · builder ---- */
    // 联网实时条到此收摊，之后的进度由打包侧接管
    update(&app, &state, &id, |t| {
        t.stage = Some(PipelineStage::Builder);
        t.progress = 84;
        t.activity = None;
    }, true);
    let output_name = output_name_of(&pack.file_name);
    let readme = build_readme(
        &plan,
        &counts,
        &review,
        parsed.manifest.loader,
        &options.keep_dirs,
        options.agree_eula,
        installed.is_some(),
    );
    let build_state = state.clone();
    let build_app = app.clone();
    // 本次包的输出目录覆写：空则回落全局设置
    let output_dir = if options.output_override.trim().is_empty() {
        PathBuf::from(&settings.output_dir)
    } else {
        PathBuf::from(options.output_override.trim())
    };
    let build_options = options.clone();
    let build_loader = parsed.manifest.loader;
    let build_id = id.clone();
    let build_output_name = output_name.clone();
    let staging_path = staging.clone();
    // 本任务上一轮的产物：同名时覆写自己那份，不去抢别人占用的文件名
    let own_output = state
        .inner
        .lock()
        .unwrap()
        .tasks
        .get(&id)
        .and_then(|t| t.output_path.clone());
    let build_cancel = cancel.clone();
    // 自检开关：打包一结束、staging 还没回收时对账产物（离线六项，零子进程）
    let verify_on = settings.verify_after_build;
    let build_installed = installed.take();
    let build_result = tokio::task::spawn_blocking(move || {
        let mut agg = ZipActivity::default();
        let (a, s, i) = (build_app, build_state, build_id);
        let input = BuildInput {
            staging: &staging,
            output_dir: &output_dir,
            output_file_name: build_output_name,
            own_output,
            options: &build_options,
            loader: build_loader,
            server_jar_name,
            installer_jar_name,
            installed: build_installed.as_ref(),
            readme_lines: readme,
        };
        // 打包是 CPU + 磁盘活，进度事件按窗口节流；取消后不再写任务表
        let built = builder::build(&input, &mut |ev| {
            if build_cancel.load(Ordering::Relaxed) {
                return;
            }
            match ev {
                BuildEvent::Plan { files, bytes } => {
                    agg.plan(*files, *bytes);
                    let snap = agg.snapshot();
                    update(&a, &s, &i, |t| t.activity = Some(snap), true);
                }
                BuildEvent::File { group, bytes } => {
                    if let Some(snap) = agg.file(group, *bytes) {
                        let (done, total) = (snap.done_bytes, snap.total_bytes);
                        update(
                            &a,
                            &s,
                            &i,
                            move |t| {
                                t.activity = Some(snap);
                                t.progress = build_progress(done, total);
                            },
                            true,
                        );
                    }
                }
                BuildEvent::Group { label, files, bytes } => {
                    let snap = agg.snapshot();
                    let (done, total) = (snap.done_bytes, snap.total_bytes);
                    log_update(
                        &a,
                        &s,
                        &i,
                        PipelineStage::Builder,
                        LogLevel::Info,
                        &format!("已打包 · {label} · {files} 个 · {}", fmt_size(*bytes)),
                        move |t| {
                            t.activity = Some(snap);
                            t.progress = build_progress(done, total);
                        },
                    );
                }
            }
        })?;
        let checks = if verify_on {
            verify::run(&verify::Input {
                staging: &staging,
                options: &build_options,
                loader: build_loader,
                plan: &plan,
                // 认 builder 的结论而不是取件时那个 jar 名：已装的包里 installer jar 根本不进包，
                // 拿旧名字对账会把「装好了」报成「启动脚本指向的 jar 不在包里」
                start_jar: built.start_jar.as_deref(),
                args_files: &built.args_files,
                installed: build_installed.is_some(),
                generated: &built.generated,
                expected_mod_files: &expected_mod_files,
                expected_keep_dirs: &kept_by_dir,
            })
        } else {
            Vec::new()
        };
        Ok::<_, BuilderError>((built, checks))
    })
    .await;

    let (built, checks) = match build_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            fail(&app, &state, &id, TaskError {
                stage: PipelineStage::Builder,
                title: "构建失败".into(),
                detail: e.to_string(),
                code: None,
                retryable: true,
                attempts: None,
                log_tail: Some(vec![e.to_string()]),
                exit_code: None,
            });
            return;
        }
        Err(e) => {
            fail(&app, &state, &id, TaskError {
                stage: PipelineStage::Builder,
                title: "构建失败".into(),
                detail: format!("构建线程异常：{e}"),
                code: None,
                retryable: true,
                attempts: None,
                log_tail: None,
                exit_code: None,
            });
            return;
        }
    };
    /* ---- 成功收尾 ---- */
    // 实际落盘名可能与默认名不同（撞名自动加了序号）：日志与报告都按实际名说
    let final_name = built
        .path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| output_name.clone());
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Builder,
        LogLevel::Info,
        &format!("生成包根文件：{}", brief_list(&built.generated)),
    );
    if built.overwritten {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            LogLevel::Info,
            &format!("覆写本任务上一次的输出：{final_name}"),
        );
    } else if final_name != output_name {
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            LogLevel::Warn,
            &format!("{output_name} 已被其他包占用，本次另存为 {final_name}"),
        );
    }
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Builder,
        LogLevel::Info,
        &format!(
            "打包 {} · {} 个文件 · {}",
            final_name,
            built.entries,
            fmt_size(built.size)
        ),
    );
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Builder,
        LogLevel::Info,
        &format!("输出路径 {}", built.path.display()),
    );
    // 自检只出一条汇总日志：逐项明细进报告卡，逐项写日志等于用日志量换看不完的字
    if !checks.is_empty() {
        let bad: Vec<&CheckResult> = checks
            .iter()
            .filter(|c| c.status != CheckStatus::Pass)
            .collect();
        let msg = if bad.is_empty() {
            format!("构建自检 {} 项 · 全部通过", checks.len())
        } else {
            let first = bad.first().unwrap();
            format!(
                "构建自检 {} 项 · {} 项需关注：{} — {}",
                checks.len(),
                bad.len(),
                first.label,
                first.detail
            )
        };
        push_log(
            &app,
            &state,
            &id,
            PipelineStage::Builder,
            if bad.is_empty() { LogLevel::Info } else { LogLevel::Warn },
            &msg,
        );
    }
    // zip 已在输出目录生成，暂存目录即刻回收（文件本体留在 cache/files 供跨任务复用）
    let _ = std::fs::remove_dir_all(&staging_path);
    let finished = now_ms();
    {
        let mut inner = state.inner.lock().unwrap();
        let t = match inner.tasks.get_mut(&id) {
            Some(t) if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) => t,
            // 已被取消：不写成功态，但实时条要收掉
            _ => {
                if let Some(t) = inner.tasks.get_mut(&id) {
                    t.activity = None;
                }
                return;
            }
        };
        t.status = TaskStatus::Success;
        t.progress = 100;
        t.stage = Some(PipelineStage::Builder);
        t.finished_at = Some(finished);
        t.activity = None;
        t.output_file_name = Some(final_name.clone());
        t.output_path = Some(built.path.clone());
        t.output_size_bytes = Some(built.size);
        push_line(
            t,
            log_line(
                PipelineStage::Builder,
                LogLevel::Info,
                &format!("打包完成 · {final_name}"),
            ),
        );
        let duration_sec = t
            .started_at
            .map(|s| ((finished - s) / 1000).max(1) as u64)
            .unwrap_or(1);
        let report = ConversionReport {
            task_id: id.clone(),
            output_file_name: final_name.clone(),
            output_size_bytes: built.size,
            duration_sec,
            removed: counts.remove,
            kept: counts.keep,
            added: counts.add,
            pending_review: review.clone(),
            options: t.options.clone(),
            file_count: built.entries as u32,
            generated_files: built.generated.clone(),
            start_jar: built.start_jar.clone(),
            installed: built.installed,
            checks,
        };
        emit_progress(&app, t);
        inner.reports.insert(id.clone(), report);
    }
    notify_done(&app, &id);
    save_tasks(&app, &state.inner.lock().unwrap());
}

/// 开了「本机安装 Loader」⇒ 官方安装器 jar 从主轮计划里拎出来提前取（阶段 2.5），
/// 关着 ⇒ 一切照旧，它仍是取件计划里的一项。判据只在这一个函数里，两家不会走偏
fn push_loader_item(
    install: bool,
    items: &mut Vec<ItemSpec>,
    first: &mut Option<ItemSpec>,
    spec: ItemSpec,
) {
    if install {
        *first = Some(spec);
    } else {
        items.push(spec);
    }
}

/// 单独取一枚加载器 jar（本机安装的前置件）。走同一个 `download_all`，所以重试、sha1 校验、
/// 下载缓存那套口径与主轮完全一致。实时条用**它自己的**账本（`items_total = 1`）：
/// 拿整包的计划量当分母会把「十几 MB 的安装器」画成「整包下好了 5%」。
async fn prefetch_loader_jar(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    dl: &mut Downloader,
    cancel: &Arc<AtomicBool>,
    spec: ItemSpec,
) -> Result<(), DownloadError> {
    let agg = Arc::new(Mutex::new(NetActivity::new(spec.size_bytes, 1)));
    let (app_t, state_t, id_t) = (app.clone(), state.clone(), id.to_string());
    let (agg_t, cancel_t) = (agg.clone(), cancel.clone());
    dl.set_transfer(Arc::new(move |p: &TransferProgress| {
        if cancel_t.load(Ordering::Relaxed) {
            return;
        }
        // 锁顺序照旧：先账本、后任务表
        if let Some(info) = agg_t.lock().unwrap().record(p, 0) {
            update(&app_t, &state_t, &id_t, |t| t.activity = Some(info), true);
        }
    }));
    let (app_f, state_f, id_f) = (app.clone(), state.clone(), id.to_string());
    let cancel_f = cancel.clone();
    let res = dl
        .download_all(vec![spec], cancel.clone(), move |done, tot, oc| {
            if cancel_f.load(Ordering::Relaxed) || done < tot {
                return;
            }
            // 一项的爬坡没有意义（只有落位那一刻），总条交给 30→34 这一步，
            // 途中那十几 MB 由实时条按真实字节走
            let _ = done;
            let (a, s, i) = (app_f.clone(), state_f.clone(), id_f.clone());
            let line = format!("安装器就位：{} · {}", oc.file_name, fmt_size(oc.bytes));
            log_update(
                &a,
                &s,
                &i,
                PipelineStage::Downloader,
                if oc.retries > 0 { LogLevel::Warn } else { LogLevel::Info },
                &line,
                move |t| t.progress = t.progress.max(LOADER_JAR_TO),
            );
        })
        .await;
    if res.is_ok() {
        // 预取的这一项不属于主轮账本，实时条到此收摊
        update(app, state, id, |t| t.activity = None, true);
    }
    res
}

/// 阶段 2.5 的入参：一次本机安装需要知道的全部坐标（都在任务快照里，不回头读全局设置）
struct InstallStage<'a> {
    loader: LoaderKind,
    mc_version: &'a str,
    loader_version: &'a str,
    /// 本次转换的 Java 需求线（`options.java_version`）：只当筛子，决定"够不够"，不决定用哪一枚
    java_required: &'a str,
    /// 用户在转换页手选的那枚 JDK 绝对路径（`options.java_path`）；空串 = 自动挑"够格且最低"的那一枚
    java_selected: &'a str,
    /// 设置里那颗「复用已装的 Loader」
    reuse: bool,
    cache_dir: &'a Path,
    /// 阶段 2.5 单独取到 staging 的官方 installer jar
    installer_jar: &'a Path,
    /// reuse 关时的落点（任务私有目录）
    scratch: &'a Path,
    cancel: &'a Arc<AtomicBool>,
}

/// 本机安装没走到末态的两种收场：取消不算失败（取消是用户意图，不该出错误卡）
enum InstallStop {
    Cancelled,
    Failed(TaskError),
}

/// 在本机跑 loader 官方安装器，拿到一份可用的 loader 树（进度、日志、失败卡都在这段外显）。
///
/// 三条口径：① **决定①——跑不成即任务失败**，不静默退回「产物到服务器上首启自装」那条老路
/// （那种包在国内服务器上最常卡住，而用户以为已经装好了）；② 整段放在 `spawn_blocking`：
/// 一趟实测 3~4 分钟，跑在 async 线程上会把其他任务的取消都堵住；③ Java 探测也在阻塞线程里
/// 现探（它自己起 `java -version`），且排在子进程之前——可预见的失败不该等一趟几分钟的安装。
async fn run_installer_stage(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    stage: InstallStage<'_>,
) -> Result<Installed, InstallStop> {
    update(app, state, id, |t| {
        t.stage = Some(PipelineStage::Installer);
        t.progress = INSTALL_FROM;
        t.activity = None;
    }, true);

    let InstallStage {
        loader,
        mc_version,
        loader_version,
        java_required,
        java_selected,
        reuse,
        cache_dir,
        installer_jar,
        scratch,
        cancel,
    } = stage;
    let (a, s, i) = (app.clone(), state.clone(), id.to_string());
    let (mc, ver, java_req, java_sel) = (
        mc_version.to_string(),
        loader_version.to_string(),
        java_required.to_string(),
        java_selected.trim().to_string(),
    );
    let (cache, jar, scratch_dir) = (cache_dir.to_path_buf(), installer_jar.to_path_buf(), scratch.to_path_buf());
    let cancel_flag = cancel.clone();
    // 实时条与日志都认这一个名字：安装器没有「正在第几个包」的接口，主体只能说到这一档为止
    let subject = format!("{} {}-{}", loader.as_label(), mc, ver);

    let joined = tokio::task::spawn_blocking(move || -> Result<Installed, InstallStop> {
        let probe = java::probe(&Some(java_req), &Some(java_sel));
        let Some(found) = probe.java_path else {
            return Err(InstallStop::Failed(TaskError {
                stage: PipelineStage::Installer,
                title: "本机没有可用的 Java".into(),
                detail: probe.detail,
                code: None,
                retryable: true,
                attempts: None,
                log_tail: None,
                exit_code: None,
            }));
        };
        let java = PathBuf::from(&found);
        // 快照里那枚已经不在这台机器上了（卸载、换盘符、换机）：退回自动那一枚继续跑，
        // 但要把换过说在日志里 —— 不然报告写着 Java 21、日志用的却是另一枚，查起来两头对不上
        if probe.selected_missing {
            push_log(
                &a,
                &s,
                &i,
                PipelineStage::Installer,
                LogLevel::Warn,
                "方案里指定的那枚 Java 已不在本机，改用自动挑到的那一枚",
            );
        }
        push_log(
            &a,
            &s,
            &i,
            PipelineStage::Installer,
            LogLevel::Info,
            &format!(
                "本机安装 {subject} · Java {} · {found}",
                probe.major.map(|m| m.to_string()).unwrap_or_else(|| "?".into())
            ),
        );

        let started = Instant::now();
        let input = installer::EnsureInput {
            loader,
            mc_version: &mc,
            loader_version: &ver,
            reuse,
            cache_dir: &cache,
            scratch: &scratch_dir,
            java: &java,
            installer_jar: &jar,
            cancel: &cancel_flag,
            timeout: installer::INSTALL_TIMEOUT,
        };
        // 安装器侧每 500ms 才扫一次目录，节拍已经比取件慢一个量级，不再另攒窗口
        let mut on_event = |ev: InstallEvent| match ev {
            InstallEvent::Log(line) => {
                push_log(&a, &s, &i, PipelineStage::Installer, LogLevel::Info, line)
            }
            InstallEvent::Progress { files, bytes } => {
                let secs = started.elapsed().as_secs_f64();
                let info = ActivityInfo {
                    kind: ActivityKind::Install,
                    subject: subject.clone(),
                    done_bytes: bytes,
                    // 总量未知（安装器不报总量）：实时条据此走不定态，不假装快满了
                    total_bytes: 0,
                    items_done: files.min(u32::MAX as u64) as u32,
                    items_total: 0,
                    rate_bps: if secs > 0.0 { bytes as f64 / secs } else { 0.0 },
                    attempt: 1,
                };
                update(
                    &a,
                    &s,
                    &i,
                    move |t| {
                        t.activity = Some(info);
                        // 总条只按估算爬坡，且只往上走：复用命中那一拍直接给末态数字
                        let p = install_progress(bytes);
                        if p > t.progress {
                            t.progress = p;
                        }
                    },
                    true,
                );
            }
        };

        match installer::ensure(&input, &mut on_event) {
            Ok(v) => {
                push_log(
                    &a,
                    &s,
                    &i,
                    PipelineStage::Installer,
                    LogLevel::Info,
                    &install_done_line(&v, &subject),
                );
                // 落点要说出来：复用模式下这里就是缓存桶的坐标，用户排查「装到哪去了」只靠这一行
                push_log(
                    &a,
                    &s,
                    &i,
                    PipelineStage::Installer,
                    LogLevel::Info,
                    &format!("安装目录 {}", v.dir.display()),
                );
                update(&a, &s, &i, |t| t.progress = INSTALL_TO, true);
                Ok(v)
            }
            Err(installer::InstallError::Cancelled) => Err(InstallStop::Cancelled),
            Err(e) => Err(InstallStop::Failed(install_task_error(&e))),
        }
    })
    .await;

    match joined {
        Ok(r) => r,
        // 阻塞线程本身炸了（内部 panic）：与构建侧同款口径报失败，不让人对着一个不动的进度猜
        Err(e) => Err(InstallStop::Failed(TaskError {
            stage: PipelineStage::Installer,
            title: "本机安装异常".into(),
            detail: format!("安装线程异常：{e}"),
            code: None,
            retryable: true,
            attempts: None,
            log_tail: None,
            exit_code: None,
        })),
    }
}

/// 安装成功的收场行：命中复用与真装出来的说法必须分开——前者一次进程都没起。
/// 顶层布局也写进来：第 6 步的启动脚本三分叉就是按「有没有 run 脚本 / 顶层是不是散 jar」判的，
/// 现在让它先在日志里可见，装错了能在这一行看出来
fn install_done_line(installed: &Installed, subject: &str) -> String {
    let counts = format!(
        "{} 个文件 · {}",
        installed.report.files,
        fmt_size(installed.report.bytes)
    );
    let layout = describe_layout(&installed.report.scripts, &installed.report.jars);
    if installed.from_cache {
        format!("复用已装的 {subject} · {counts} · 未起进程 · {layout}")
    } else {
        format!(
            "本机安装完成 · {subject} · {counts} · 用时 {:.1} 秒 · {layout}",
            installed.report.elapsed.as_secs_f64()
        )
    }
}

fn describe_layout(scripts: &[String], jars: &[String]) -> String {
    let head = if scripts.is_empty() { None } else { Some(scripts.join("、")) };
    let tail = if jars.is_empty() { None } else { Some(jars.join("、")) };
    match (head, tail) {
        (Some(s), None) => format!("顶层 {s}"),
        (Some(s), Some(j)) => format!("顶层 {s} + {j}"),
        (None, Some(j)) => format!("无 run 脚本，顶层散 jar {j}"),
        (None, None) => "顶层既无 run 脚本也无散 jar".to_string(),
    }
}

/// 安装失败 → 错误卡载荷。退出码在安装器那边是字符串（Windows 与 Unix 口径不同），
/// 留在 detail 里说，不硬塞进 `TaskError.exit_code` 那个 i32 槽
fn install_task_error(e: &installer::InstallError) -> TaskError {
    let title = match e {
        installer::InstallError::Io(_) => "安装目录准备失败",
        installer::InstallError::Spawn(_) => "Java 起不来",
        installer::InstallError::Timeout { .. } => "本机安装超时",
        installer::InstallError::Failed { .. } => "安装器报错",
        installer::InstallError::Incomplete { .. } => "安装器报成功却没装出结果",
        installer::InstallError::Cancelled => "本机安装已取消",
    };
    let mut detail = e.to_string();
    if matches!(e, installer::InstallError::Timeout { .. }) {
        detail.push_str("，安装器进程已终止，半成品已回收");
    }
    TaskError {
        stage: PipelineStage::Installer,
        title: title.into(),
        detail,
        code: None,
        retryable: true,
        attempts: None,
        log_tail: None,
        exit_code: None,
    }
}

fn map_download_error(app: &AppHandle, state: &Arc<AppState>, id: &str, e: &DownloadError) {
    // `detail` 是带 URL 与状态码的原句（只进「复制诊断信息」），`code` 才是界面上那句话的种类
    let code = e.net_code();
    let (title, detail, attempts) = match e {
        DownloadError::Failed {
            file_name,
            attempts,
            cause,
        } => (
            // attempts>0 才是真联网重试；包内/本地读失败不叫「下载失败」
            if *attempts > 0 {
                "联网下载失败".to_string()
            } else {
                "文件获取失败".to_string()
            },
            format!("{file_name} — {cause}"),
            Some(*attempts),
        ),
        DownloadError::NotFound(s) => ("依赖解析失败".to_string(), s.clone(), None),
        other => ("取件失败".to_string(), other.to_string(), None),
    };
    fail(
        app,
        state,
        id,
        TaskError {
            stage: PipelineStage::Downloader,
            title,
            detail,
            code,
            retryable: true,
            attempts,
            log_tail: None,
            exit_code: None,
        },
    );
}

fn build_readme(
    plan: &[PlanMod],
    counts: &PlanCounts,
    review: &[String],
    loader: LoaderKind,
    keep_dirs: &[String],
    agree_eula: bool,
    installed: bool,
) -> Vec<String> {
    // 最后一维是「阶段 2.5 有没有在本机把 loader 装好并进了包」：README 的启动说法按它分叉
    let loader_line = match (loader, installed) {
        // 关着这一档才是原来那句：包里只有 installer，首次运行联网自装
        (LoaderKind::Forge | LoaderKind::NeoForge, false) => {
            "Forge/NeoForge：start 脚本首次运行会自动执行 installServer（需要本机 Java 与网络），届时生成 run.bat/run.sh 与服务器本体，之后以 run 脚本启动".to_string()
        }
        (LoaderKind::Forge | LoaderKind::NeoForge, true) => {
            "Forge/NeoForge：加载器与依赖已在本机装好并打进包，解压后直接运行 start.bat / start.sh，无需联网安装".to_string()
        }
        (LoaderKind::Fabric, _) => {
            // Fabric 没有 installer 可提前跑：包里那枚官方服务端 jar 自己是启动器，首启现拉 loader 与前置库
            // （实测见 `.scratch/installer-probe`）。写"直接运行"会让人把那段联网等待当成卡死
            "Fabric 服务端：start 脚本首次运行会联网装出加载器与前置库（需要本机 Java 与网络），之后同样以该脚本启动".to_string()
        }
    };
    let mut lines = vec![
        "SideShift 转换报告".to_string(),
        format!("剔除 {} · 保留 {} · 新增 {}", counts.remove, counts.keep, counts.add),
        loader_line,
    ];
    if !agree_eula {
        lines.push("eula.txt 已生成但为 eula=false：首次启动前请改为 eula=true，否则服务端会拒绝启动".to_string());
    }
    if !keep_dirs.is_empty() {
        lines.push(format!("已随包保留客户端目录：{}", keep_dirs.join("、")));
    }
    if !review.is_empty() {
        lines.push(format!("待人工确认模组：{}", review.join("、")));
    }
    let removed: Vec<String> = plan
        .iter()
        .filter(|m| m.disposition == ModDisposition::Remove)
        .map(|m| m.name.clone())
        .collect();
    if !removed.is_empty() {
        lines.push(format!("已剔除客户端模组 {} 个", removed.len()));
    }
    lines
}

fn output_name_of(file_name: &str) -> String {
    let stem = file_name
        .strip_suffix(".mrpack")
        .or_else(|| file_name.strip_suffix(".zip"))
        .or_else(|| file_name.strip_suffix(".7z"))
        .unwrap_or(file_name);
    format!("{stem}-server.zip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn installed(from_cache: bool, scripts: &[&str], jars: &[&str], secs: u64) -> Installed {
        Installed {
            dir: PathBuf::from("X:\\cache\\installs\\forge\\1.20.1-47.4.10"),
            from_cache,
            report: installer::InstallReport {
                files: 107,
                bytes: 158 * 1024 * 1024,
                elapsed: Duration::from_secs(secs),
                scripts: scripts.iter().map(|s| s.to_string()).collect(),
                jars: jars.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    /// 收场行：复用与真装的说法分开（前者一次进程都没起），耗时只出现在真装过的那条上
    #[test]
    fn install_done_line_separates_cache_hit_from_real_run() {
        let hit = install_done_line(&installed(true, &["run.bat", "run.sh"], &[], 0), "Forge 1.20.1-47.4.10");
        assert!(hit.starts_with("复用已装的 Forge 1.20.1-47.4.10"), "{hit}");
        assert!(hit.contains("未起进程"), "命中必须说明零进程，否则读起来像装完了");
        assert!(!hit.contains("用时"), "命中没有耗时可报");

        let real = install_done_line(&installed(false, &["run.bat", "run.sh"], &[], 226), "Forge 1.20.1-47.4.10");
        assert!(real.contains("本机安装完成") && real.contains("226.0 秒"), "{real}");
        assert!(real.contains("顶层 run.bat、run.sh"), "{real}");
    }

    /// 老 Forge（≤1.16）装完没有 run 脚本、只有顶层散 jar：这条日志就是第 6 步三分叉的判据，
    /// 说法必须与真实布局对上，不能把「没脚本」写成成功
    #[test]
    fn install_done_line_names_the_layout_it_found() {
        let v = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let old = describe_layout(&v(&[]), &v(&["forge-1.16.5-36.2.39.jar", "minecraft_server.1.16.5.jar"]));
        assert!(old.starts_with("无 run 脚本，顶层散 jar "), "{old}");
        assert_eq!(describe_layout(&v(&["run.sh"]), &v(&["a.jar"])), "顶层 run.sh + a.jar");
        assert_eq!(describe_layout(&v(&[]), &v(&[])), "顶层既无 run 脚本也无散 jar");
    }

    /// 取消不算失败：错误卡只由 Failed 分支生成，取消走的是与取件同款的中止日志
    #[test]
    fn install_failures_are_retryable_and_keep_the_cause() {
        let e = install_task_error(&installer::InstallError::Failed {
            code: "1".into(),
            tail: "安装器报告成功 / There was an error during installation".into(),
        });
        assert_eq!(e.stage, PipelineStage::Installer);
        assert!(e.retryable, "装 JDK 之后点重试就该能过，不能判成死路");
        assert!(e.detail.contains("退出码 1"), "{}", e.detail);
        assert!(e.detail.contains("There was an error"), "安装器的尾巴要跟着进错误卡，否则只剩一句空话");

        let t = install_task_error(&installer::InstallError::Timeout { secs: 1800 });
        assert!(t.detail.contains("半成品已回收"), "{}", t.detail);
        // 安装器退出码不是 i32 口径（Windows/Unix 不同），不硬塞进 exit_code 槽
        assert_eq!(e.exit_code, None);
    }
}

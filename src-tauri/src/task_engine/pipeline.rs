//! 四阶段流水线：parser ≤15 · detector ≤30 · downloader ≤82 · builder ≤100。
//! 阶段进度口径与前端 mock 引擎一致。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::AppHandle;

use crate::core::builder::{self, BuildEvent, BuildInput, BuilderError};
use crate::core::detector;
use crate::core::downloader::{
    DownloadError, Downloader, Fetch, FetchSource, ItemSpec, TransferProgress,
};
use crate::core::parser::{self, ParsedPack};
use crate::core::verify;
use crate::models::*;
use super::activity::{
    apply_fetch_counts, build_progress, fetch_group_label, flush_groups, group_line,
    group_plan_of, reports_per_line, FetchGroups, NetActivity, ZipActivity,
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
    let dl = Downloader::new(
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
            items.push(spec);
        }
        LoaderKind::NeoForge => {
            let mut spec = dl.neoforge_installer(&options.loader_version);
            dl.attach_side_sha1(&mut spec).await;
            installer_jar_name = Some(spec.file_name.clone());
            spec.dest = staging.join(&spec.file_name);
            items.push(spec);
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
    let cancel = state
        .inner
        .lock()
        .unwrap()
        .cancel
        .get(&id)
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
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
                        apply_fetch_counts(t, done, tot, &net_u, &bytes_u);
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
                        move |t| apply_fetch_counts(t, g.done, g.total, &net_u, &bytes_u),
                    );
                }
                None => {
                    if groups_f.lock().unwrap().beat_due() {
                        let (net_u, bytes_u) = (net_f.clone(), bytes_f.clone());
                        update(
                            &app_f,
                            &state_f,
                            &id_f,
                            |t| apply_fetch_counts(t, done, tot, &net_u, &bytes_u),
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
        flush_groups(&app, &state, &id, &groups, &net_actual, &bytes_actual);
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
    let readme = build_readme(&plan, &counts, &review, parsed.manifest.loader, &options.keep_dirs, options.agree_eula);
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
    let build_start_jar = loader_jar.clone();
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
                start_jar: build_start_jar.as_deref(),
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
            checks,
        };
        emit_progress(&app, t);
        inner.reports.insert(id.clone(), report);
    }
    notify_done(&app, &id);
    save_tasks(&app, &state.inner.lock().unwrap());
}

fn map_download_error(app: &AppHandle, state: &Arc<AppState>, id: &str, e: &DownloadError) {
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
) -> Vec<String> {
    let mut lines = vec![
        "SideShift 转换报告".to_string(),
        format!("剔除 {} · 保留 {} · 新增 {}", counts.remove, counts.keep, counts.add),
        match loader {
            LoaderKind::Fabric => "Fabric 服务端：直接运行 start.bat / start.sh".to_string(),
            LoaderKind::Forge | LoaderKind::NeoForge => {
                "Forge/NeoForge：start 脚本首次运行会自动执行 installServer（需要本机 Java 与网络），届时生成 run.bat/run.sh 与服务器本体，之后以 run 脚本启动".to_string()
            }
        },
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

//! 任务引擎：内存任务注册表 + 四阶段流水线调度 + 取消 + 进度事件。
//! 阶段进度口径与前端 mock 引擎一致：parser≤15 · detector≤30 · downloader≤82 · builder≤100。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::core::builder::{self, BuildInput};
use crate::core::detector::{self, split_mod_file};
use crate::core::downloader::{DownloadError, Downloader, Fetch, ItemSpec};
use crate::core::parser::{self, ParsedPack};
use crate::models::*;

pub const EVENT_PROGRESS: &str = "conversion://progress";
pub const EVENT_DONE: &str = "conversion://done";
const SETTINGS_FILE: &str = "settings.json";
/// 任务本地存档：注册表全量快照（任务 + 方案 + 报告），重启后可见可重试
const TASKS_FILE: &str = "tasks.json";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct TasksFile {
    #[serde(default)]
    tasks: HashMap<String, ConversionTask>,
    #[serde(default)]
    plans: HashMap<String, Vec<PlanMod>>,
    #[serde(default)]
    reports: HashMap<String, ConversionReport>,
}

#[derive(Default)]
pub struct Inner {
    pub tasks: HashMap<String, ConversionTask>,
    pub cancel: HashMap<String, Arc<AtomicBool>>,
    pub reports: HashMap<String, ConversionReport>,
    /// 已解析包缓存：按包文件名索引（start_conversion 由 manifest.fileName 找回）
    pub parsed_by_name: HashMap<String, Arc<ParsedPack>>,
    /// 最近一次成功解析的包名（get_plan / default_options 的默认对象）
    pub last_file: Option<String>,
    /// 任务创建时前端确认过的最终方案（含用户勾改/本地与服务端新增）
    pub plans: HashMap<String, Vec<PlanMod>>,
    pub settings: AppSettings,
    /// 当前独占运行的任务 id——同一时间只允许一条转换在跑，其余排队
    pub current: Option<String>,
}

pub struct AppState {
    pub inner: Mutex<Inner>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Self {
        let mut inner = Inner {
            settings: load_settings(app),
            ..Default::default()
        };
        load_tasks(app, &mut inner);
        Self {
            inner: Mutex::new(inner),
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/* ---------------- 设置持久化 ---------------- */

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(SETTINGS_FILE))
}

pub fn load_settings(app: &AppHandle) -> AppSettings {
    let home = app
        .path()
        .home_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let defaults = AppSettings::defaults_for(&home);
    let saved = settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok());
    match saved {
        // 空目录字段回落默认值（用户清空输入框的场景）
        Some(s) => AppSettings {
            output_dir: if s.output_dir.is_empty() { defaults.output_dir.clone() } else { s.output_dir },
            cache_dir: if s.cache_dir.is_empty() { defaults.cache_dir.clone() } else { s.cache_dir },
            ..s
        },
        None => defaults,
    }
}

pub fn save_settings(app: &AppHandle, s: &AppSettings) {
    if let Some(p) = settings_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(s) {
            let _ = std::fs::write(p, json);
        }
    }
}

/* ---------------- 任务存档 ---------------- */

fn tasks_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(TASKS_FILE))
}

/// 启动回灌：tasks/plans/reports 全量恢复；上次会话遗留的排队/运行中转会失败可重试
/// （流水线、源包解析缓存都不跨进程，复活即错）
fn load_tasks(app: &AppHandle, inner: &mut Inner) {
    let Some(Ok(text)) = tasks_path(app).map(|f| std::fs::read_to_string(f)) else { return };
    let Ok(v) = serde_json::from_str::<TasksFile>(&text) else { return };
    for (id, mut t) in v.tasks {
        if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) {
            t.status = TaskStatus::Failed;
            t.error = Some(TaskError {
                stage: t.stage.unwrap_or(PipelineStage::Parser),
                title: "转换中断".into(),
                detail: "应用退出时任务尚未完成，可重试".into(),
                retryable: true,
                attempts: None,
                log_tail: None,
                exit_code: None,
            });
            t.finished_at = Some(now_ms());
        }
        inner.tasks.insert(id, t);
    }
    inner.plans = v.plans;
    inner.reports = v.reports;
}

/// 注册表任意变更后同步落盘（量小、低频，调用方持锁即可）
pub fn save_tasks(app: &AppHandle, inner: &Inner) {
    let Some(p) = tasks_path(app) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let snapshot = TasksFile {
        tasks: inner.tasks.clone(),
        plans: inner.plans.clone(),
        reports: inner.reports.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(p, json);
    }
}

/* ---------------- 任务创建与调度 ---------------- */

/// 任务创建结果：queued = 已有任务在跑，本条进入排队队列
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub task_id: String,
    pub queued: bool,
}

/// 注册任务，空闲则立即开跑，否则留在排队队列，返回 (id, 是否排队)。
/// 方案由前端 Convert 页确认后整体传入（含勾改与新增项）。
pub fn create_task(
    app: &AppHandle,
    state: &Arc<AppState>,
    options: ConversionOptions,
    manifest: PackManifest,
    plan: Vec<PlanMod>,
) -> StartResult {
    let id = format!("task-{}", uuid::Uuid::new_v4().simple());
    let task = ConversionTask {
        id: id.clone(),
        pack: manifest,
        options,
        status: TaskStatus::Queued,
        stage: None,
        progress: 0,
        downloaded: None,
        total: None,
        created_at: now_ms(),
        started_at: None,
        finished_at: None,
        error: None,
        counts: None,
        output_file_name: None,
        output_size_bytes: None,
        logs: Vec::new(),
    };
    let queued = {
        let mut inner = state.inner.lock().unwrap();
        inner.plans.insert(id.clone(), plan);
        inner.cancel.insert(id.clone(), Arc::new(AtomicBool::new(false)));
        inner.tasks.insert(id.clone(), task);
        save_tasks(app, &inner);
        // 有任务在跑就安心排队；否则本条即刻上位
        if inner.current.is_some() {
            true
        } else {
            inner.current = Some(id.clone());
            false
        }
    };
    if !queued {
        spawn_pipeline(app, state, id.clone());
    }
    StartResult { task_id: id, queued }
}

/// 独占槽位交接点：流水线退出（成功/失败/取消皆如此）后拉起队首排队任务。
/// 返回 Some(next_id) 表示调用方需 spawn
fn release_and_next(app: &AppHandle, state: &Arc<AppState>, done_id: &str) -> Option<String> {
    let next = {
        let mut inner = state.inner.lock().unwrap();
        if inner.current.as_deref() == Some(done_id) {
            inner.current = None;
        }
        if inner.current.is_some() {
            None
        } else {
            let cand = inner
                .tasks
                .iter()
                .filter(|(_, t)| t.status == TaskStatus::Queued)
                .min_by_key(|(_, t)| t.created_at)
                .map(|(id, _)| id.clone());
            if cand.is_some() {
                inner.current = cand.clone();
            }
            cand
        }
    };
    save_tasks(app, &state.inner.lock().unwrap());
    next
}

fn spawn_pipeline(app: &AppHandle, state: &Arc<AppState>, id: String) {
    let (a, s) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        run_pipeline(a.clone(), s.clone(), id.clone()).await;
        if let Some(next) = release_and_next(&a, &s, &id) {
            spawn_pipeline(&a, &s, next);
        }
    });
}

pub fn cancel(app: &AppHandle, state: &Arc<AppState>, id: &str) {
    {
        let mut inner = state.inner.lock().unwrap();
        if let Some(flag) = inner.cancel.get(id) {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(t) = inner.tasks.get_mut(id) {
            if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) {
                t.status = TaskStatus::Cancelled;
                t.finished_at = Some(now_ms());
                let stage = t.stage.unwrap_or(PipelineStage::Parser);
                t.logs.push(log_line(stage, LogLevel::Warn, "任务已被用户取消"));
            }
        }
        save_tasks(app, &inner);
    }
    // 排队中的任务从未开跑，取消后不会再有收尾钩子，这里直接放行下一队
    if state.inner.lock().unwrap().current.as_deref() != Some(id) {
        if let Some(next) = release_and_next(app, state, id) {
            spawn_pipeline(app, state, next);
        }
    }
}

/* ---------------- 进度更新与事件 ---------------- */

fn log_line(stage: PipelineStage, level: LogLevel, message: &str) -> TaskLogLine {
    TaskLogLine {
        time: now_hms(),
        stage,
        message: message.to_string(),
        level,
    }
}

fn emit_progress(app: &AppHandle, t: &ConversionTask) {
    let ev = ProgressEvent {
        task_id: t.id.clone(),
        stage: t.stage.unwrap_or(PipelineStage::Parser),
        progress: t.progress,
        downloaded: t.downloaded,
        total: t.total,
        log: t.logs.last().cloned(),
    };
    let _ = app.emit(EVENT_PROGRESS, ev);
}

/// 更新任务并（可选）广播进度事件
fn update(app: &AppHandle, state: &Arc<AppState>, id: &str, f: impl FnOnce(&mut ConversionTask), emit: bool) {
    let mut inner = state.inner.lock().unwrap();
    if let Some(t) = inner.tasks.get_mut(id) {
        f(t);
        if emit {
            emit_progress(app, t);
        }
    }
}

fn push_log(app: &AppHandle, state: &Arc<AppState>, id: &str, stage: PipelineStage, level: LogLevel, msg: &str) {
    update(
        app,
        state,
        id,
        |t| {
            t.logs.push(log_line(stage, level, msg));
        },
        true,
    );
}

fn is_active(state: &Arc<AppState>, id: &str) -> bool {
    let inner = state.inner.lock().unwrap();
    matches!(
        inner.tasks.get(id),
        Some(t) if matches!(t.status, TaskStatus::Queued | TaskStatus::Running)
    )
}

fn fail(app: &AppHandle, state: &Arc<AppState>, id: &str, error: TaskError) {
    update(
        app,
        state,
        id,
        |t| {
            // 已取消的行不再转失败（取消与下载/构建报错可能同时到达）
            if !matches!(t.status, TaskStatus::Queued | TaskStatus::Running) {
                return;
            }
            t.status = TaskStatus::Failed;
            t.error = Some(error.clone());
            t.finished_at = Some(now_ms());
            t.logs.push(log_line(
                error.stage,
                LogLevel::Error,
                &format!("{} · {}", error.title, error.detail),
            ));
        },
        true,
    );
    let _ = app.emit(EVENT_DONE, HashMap::from([("taskId".to_string(), id.to_string())]));
    save_tasks(app, &state.inner.lock().unwrap());
}

/* ---------------- 流水线 ---------------- */

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
    update(&app, &state, &id, |t| t.progress = 15, false);
    if !is_active(&state, &id) {
        return;
    }

    /* ---- 阶段 2 · detector ---- */
    update(&app, &state, &id, |t| t.stage = Some(PipelineStage::Detector), false);
    let counts = detector::count_plan(&plan);
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Detector,
        LogLevel::Info,
        &format!("方案确认：剔除 {} · 保留 {} · 新增 {}", counts.remove, counts.keep, counts.add),
    );
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
        .join("tasks")
        .join(&id)
        .join("staging");
    let _ = std::fs::remove_dir_all(&staging);
    let mods_dir = staging.join("mods");
    let dl = Downloader::new(
        PathBuf::from(&settings.cache_dir),
        settings.concurrency as usize,
    );
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
            items.push(ItemSpec {
                fetch: Fetch::Url(p.url.clone()),
                sha1: p.sha1.clone(),
                dest: mods_dir.join(&file_name),
                file_name,
                size_bytes: row.size_bytes,
            });
            continue;
        }
        let matched = parsed
            .mod_files
            .iter()
            .enumerate()
            .find(|(i, f)| !used_files.contains(i) && split_mod_file(&f.file_name).0 == row.id);
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
    for f in &parsed.extra_files {
        let rel = f.path.replace('\\', "/");
        let logical = parser::logical_rel(&rel);
        let lower = logical.to_lowercase();
        let keep = options
            .keep_dirs
            .iter()
            .any(|d| lower.starts_with(&format!("{}/", d.to_lowercase())));
        if !keep {
            continue;
        }
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

    let total = items.len() as u32;
    update(&app, &state, &id, |t| {
        t.total = Some(total);
        t.downloaded = Some(0);
    }, false);
    push_log(
        &app,
        &state,
        &id,
        PipelineStage::Downloader,
        LogLevel::Info,
        &format!("准备下载 {total} 个文件 · 并发 {}", settings.concurrency),
    );

    let app2 = app.clone();
    let state2 = state.clone();
    let id2 = id.clone();
    let cancel = state
        .inner
        .lock()
        .unwrap()
        .cancel
        .get(&id)
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let dl_result = dl
        .download_all(items, cancel, move |done, tot| {
            update(
                &app2,
                &state2,
                &id2,
                |t| {
                    t.downloaded = Some(done as u32);
                    t.total = Some(tot as u32);
                    t.progress = 30 + (52 * done as u32).checked_div(tot as u32).unwrap_or(0).max(1);
                },
                true,
            );
        })
        .await;
    if let Err(e) = dl_result {
        map_download_error(&app, &state, &id, &e);
        return;
    }
    if !is_active(&state, &id) {
        push_log(&app, &state, &id, PipelineStage::Downloader, LogLevel::Warn, "任务已取消，下载中止");
        return;
    }
    update(&app, &state, &id, |t| t.progress = 82, true);

    /* ---- 阶段 4 · builder ---- */
    update(&app, &state, &id, |t| {
        t.stage = Some(PipelineStage::Builder);
        t.progress = 84;
    }, true);
    let output_name = output_name_of(&pack.file_name);
    let review: Vec<String> = plan
        .iter()
        .filter(|m| m.needs_review)
        .map(|m| m.name.clone())
        .collect();
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
    let build_result = tokio::task::spawn_blocking(move || {
        update(&build_app, &build_state, &build_id, |t| t.progress = 90, false);
        builder::build(&BuildInput {
            staging: &staging,
            output_dir: &output_dir,
            output_file_name: build_output_name,
            options: &build_options,
            loader: build_loader,
            server_jar_name,
            installer_jar_name,
            readme_lines: readme,
        })
    })
    .await;

    let (_out_path, size) = match build_result {
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
    // zip 已在输出目录生成，暂存目录即刻回收（文件本体留在 cache/files 供跨任务复用）
    let _ = std::fs::remove_dir_all(&staging_path);
    let finished = now_ms();
    {
        let mut inner = state.inner.lock().unwrap();
        let t = match inner.tasks.get_mut(&id) {
            Some(t) if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) => t,
            _ => return, // 已被取消：不写成功态
        };
        t.status = TaskStatus::Success;
        t.progress = 100;
        t.stage = Some(PipelineStage::Builder);
        t.finished_at = Some(finished);
        t.output_file_name = Some(output_name.clone());
        t.output_size_bytes = Some(size);
        t.logs.push(log_line(
            PipelineStage::Builder,
            LogLevel::Info,
            &format!("打包完成 · {output_name}"),
        ));
        let duration_sec = t
            .started_at
            .map(|s| ((finished - s) / 1000).max(1) as u64)
            .unwrap_or(1);
        let report = ConversionReport {
            task_id: id.clone(),
            output_file_name: output_name.clone(),
            output_size_bytes: size,
            duration_sec,
            removed: counts.remove,
            kept: counts.keep,
            added: counts.add,
            pending_review: review.clone(),
            options: t.options.clone(),
        };
        emit_progress(&app, t);
        inner.reports.insert(id.clone(), report);
    }
    let _ = app.emit(EVENT_DONE, HashMap::from([("taskId".to_string(), id.clone())]));
    save_tasks(&app, &state.inner.lock().unwrap());
}

fn map_download_error(app: &AppHandle, state: &Arc<AppState>, id: &str, e: &DownloadError) {
    let (title, detail, attempts) = match e {
        DownloadError::Failed {
            file_name,
            attempts,
            cause,
        } => (
            "依赖下载失败".to_string(),
            format!("{file_name} — {cause}"),
            Some(*attempts),
        ),
        DownloadError::NotFound(s) => ("依赖解析失败".to_string(), s.clone(), None),
        other => ("依赖下载失败".to_string(), other.to_string(), None),
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

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect()
}

/// mods/ 唯一落位名：同名 jar（来自不同目录）第二份起追加方案 id，防静默覆盖
fn unique_mod_name(used: &mut HashSet<String>, name: &str, id: &str) -> String {
    if used.insert(name.to_lowercase()) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
        _ => (name.to_string(), "jar".to_string()),
    };
    let sid = sanitize(id);
    let mut candidate = format!("{stem}-{sid}.{ext}");
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{stem}-{sid}-{n}.{ext}");
        n += 1;
    }
    candidate
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

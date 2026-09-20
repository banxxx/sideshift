//! 任务引擎：内存任务注册表 + 四阶段流水线调度 + 取消 + 进度事件。
//! 阶段进度口径与前端 mock 引擎一致：parser≤15 · detector≤30 · downloader≤82 · builder≤100。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::core::builder::{self, BuildEvent, BuildInput};
use crate::core::detector;
use crate::core::downloader::{
    DownloadError, Downloader, Fetch, FetchSource, ItemSpec, TransferProgress,
};
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
        sweep_task_staging(&inner);
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
        // 旧版本没有日志上限，存档里可能躺着几千行（曾把界面卡死）——回灌时就裁掉
        trim_logs(&mut t.logs);
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
        fetch: None,
        net_done: None,
        done_bytes: None,
        activity: None,
        created_at: now_ms(),
        started_at: None,
        finished_at: None,
        error: None,
        counts: None,
        output_file_name: None,
        output_path: None,
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
/// 原地重试：复用同一 id 与同一份方案，重置运行态后重新排队。
/// 复用 id 才认得上一次的产物（outputPath）——同名时覆写自己那份，而不是每重试一次就多一个序号包。
pub fn retry_task(app: &AppHandle, state: &Arc<AppState>, id: &str) -> Option<StartResult> {
    let queued = {
        let mut inner = state.inner.lock().unwrap();
        let t = inner.tasks.get_mut(id)?;
        // 已在跑/已在队：不重复拉起，把现状原样回给前端
        if matches!(t.status, TaskStatus::Queued | TaskStatus::Running) {
            return Some(StartResult {
                task_id: id.to_string(),
                queued: inner.current.as_deref() != Some(id),
            });
        }
        t.status = TaskStatus::Queued;
        t.stage = None;
        t.progress = 0;
        t.error = None;
        t.activity = None;
        t.started_at = None;
        t.finished_at = None;
        t.downloaded = None;
        t.net_done = None;
        t.done_bytes = None;
        t.output_file_name = None;
        t.output_size_bytes = None;
        // 排队顺序按创建时间排，重试要顺延到队尾
        t.created_at = now_ms();
        push_line(
            t,
            log_line(
                PipelineStage::Parser,
                LogLevel::Info,
                "重新排队：沿用上次方案（日志保留上一轮的记录）",
            ),
        );
        // 取消标志必须复位，否则新一轮每个回调都会被它拦死
        if let Some(flag) = inner.cancel.get(id) {
            flag.store(false, Ordering::Relaxed);
        }
        save_tasks(app, &inner);
        if inner.current.is_some() {
            true
        } else {
            inner.current = Some(id.to_string());
            false
        }
    };
    if !queued {
        spawn_pipeline(app, state, id.to_string());
    }
    Some(StartResult { task_id: id.to_string(), queued })
}

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
        // 失败与取消不走成功收尾，暂存目录统一在这里回收
        remove_task_staging(&s, &id);
        if let Some(next) = release_and_next(&a, &s, &id) {
            spawn_pipeline(&a, &s, next);
        }
    });
}

/// 回收某个任务的暂存目录（cache/tasks/<id>），文件本体留在 cache/files 供跨任务复用
pub fn remove_task_staging(state: &AppState, id: &str) {
    let cache_dir = state.inner.lock().unwrap().settings.cache_dir.clone();
    let _ = std::fs::remove_dir_all(PathBuf::from(cache_dir).join("tasks").join(id));
}

/// 启动回收：上次进程被强杀时来不及清理的暂存目录（注册表里已无此任务即删）
fn sweep_task_staging(inner: &Inner) {
    let dir = PathBuf::from(&inner.settings.cache_dir).join("tasks");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !inner.tasks.contains_key(&name) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
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
                t.activity = None;
                let stage = t.stage.unwrap_or(PipelineStage::Parser);
                push_line(t, log_line(stage, LogLevel::Warn, "任务已被用户取消"));
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

/// 单任务日志上限：保留目录命中 kubejs/资源包这类目录时条目数以千计，日志不设上限会把
/// tasks.json 撑到 MB 级、并让每次进度事件都全量搬运日志给前端 —— 实测 7340 行直接把界面卡死。
/// 超限后只留最近这些条（含一条截断标记），复制与存档口径一致。
const MAX_LOG_LINES: usize = 600;

/// 超限时裁到最近 MAX_LOG_LINES 条，并在头部放一条截断说明（旧头已被裁掉，所以永远只有一条）
fn trim_logs(logs: &mut Vec<TaskLogLine>) {
    if logs.len() <= MAX_LOG_LINES {
        return;
    }
    let stage = logs.last().map(|l| l.stage).unwrap_or(PipelineStage::Parser);
    logs.drain(..logs.len() - (MAX_LOG_LINES - 1));
    logs.insert(
        0,
        log_line(
            stage,
            LogLevel::Warn,
            &format!("（日志过长，仅保留最近 {} 条）", MAX_LOG_LINES - 1),
        ),
    );
}

fn push_line(t: &mut ConversionTask, line: TaskLogLine) {
    t.logs.push(line);
    trim_logs(&mut t.logs);
}

fn emit_progress(app: &AppHandle, t: &ConversionTask) {
    let ev = ProgressEvent {
        task_id: t.id.clone(),
        stage: t.stage.unwrap_or(PipelineStage::Parser),
        progress: t.progress,
        downloaded: t.downloaded,
        total: t.total,
        log: t.logs.last().cloned(),
        activity: t.activity.clone(),
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
            push_line(t, log_line(stage, level, msg));
        },
        true,
    );
}

/// 追加日志并更新任务：同一把锁、一次事件（日志与进度成对出现时用）
fn log_update(app: &AppHandle, state: &Arc<AppState>, id: &str, stage: PipelineStage, level: LogLevel, msg: &str, f: impl FnOnce(&mut ConversionTask)) {
    update(
        app,
        state,
        id,
        |t| {
            push_line(t, log_line(stage, level, msg));
            f(t);
        },
        true,
    );
}

/// 离线取件的分组账本：整合包 / 本地 / 缓存条目成百上千，逐条打日志会同时打爆
/// 进度事件（前端每事件刷一次列表）与 tasks.json 落盘 —— 这正是"取件慢"的一半成因。
/// 口径改成一目录一条：按落位目录（模组 / 各保留目录）攒，取满该目录预设数量才出日志，
/// 取的过程中只按间隔发「不含日志」的进度心跳，保证进度条持续走动。
#[derive(Clone, Default)]
struct GroupTally {
    label: String,
    /// 本目录应取件总数，建取件计划时按与回调同一套路由口径算出
    expected: u32,
    files: u32,
    bytes: u64,
    done: usize,
    total: usize,
    flushed: bool,
}

impl GroupTally {
    fn new(label: String, expected: u32) -> Self {
        Self { label, expected, ..Default::default() }
    }
}

/// 分目录账本 + 进度心跳窗口（窗口起点记在结构体上，取走条目时不可连带清零）
#[derive(Default)]
struct FetchGroups {
    rows: Vec<GroupTally>,
    last_beat: Option<std::time::Instant>,
}

/// 距上次心跳超过这么久才推进度（日志仍只在目录取满时出）
const BEAT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

impl FetchGroups {
    fn new(plan: Vec<(String, u32)>) -> Self {
        Self {
            rows: plan
                .into_iter()
                .map(|(label, expected)| GroupTally::new(label, expected))
                .collect(),
            last_beat: None,
        }
    }

    /// 记一笔离线条目；返回 Some(快照) 表示该目录刚取满，应当出一条日志
    fn record(&mut self, label: &str, bytes: u64, done: usize, total: usize) -> Option<GroupTally> {
        if self.rows.iter().all(|r| r.label != label) {
            // 计划外的落位（理论上到不了这里）：按单条目成组，取到即算完成
            self.rows.push(GroupTally::new(label.to_string(), 1));
        }
        let r = self.rows.iter_mut().find(|r| r.label == label)?;
        r.files += 1;
        r.bytes += bytes;
        r.done = done;
        r.total = total;
        if r.flushed || r.files < r.expected {
            return None;
        }
        r.flushed = true;
        Some(r.clone())
    }

    /// 兜底：整轮取件结束后，仍有进账却没出过日志的目录各补一条（预设数对不上时不至于静默）
    fn pending(&mut self) -> Vec<GroupTally> {
        let mut out = Vec::new();
        for r in self.rows.iter_mut() {
            if !r.flushed && r.files > 0 {
                r.flushed = true;
                out.push(r.clone());
            }
        }
        out
    }

    /// 心跳是否该发，并在该发时重开窗口
    fn beat_due(&mut self) -> bool {
        let due = self
            .last_beat
            .map(|t| t.elapsed() >= BEAT_INTERVAL)
            .unwrap_or(true);
        if due {
            self.last_beat = Some(std::time::Instant::now());
        }
        due
    }
}

/// 分组完成日志（心跳日志同口径，只是不带完成结论）
fn group_line(g: &GroupTally) -> String {
    format!("取件 · {} · {} 个 · {}", g.label, g.files, fmt_size(g.bytes))
}

/// 该条目是否逐条上报。真正联网（未命中缓存）或重试过的才值得单列一条；
/// 其余（包内 / 本地 / 缓存命中）都是磁盘搬运，按落位目录攒成一条。
/// 建计划与取件回调共用此判定，两边口径不可能走偏。
fn reports_per_line(network: bool, cached: bool, retries: u32) -> bool {
    (network && !cached) || retries > 0
}

/// 分目录账本计划：每个落位目录预设取件多少条。
/// 「是否入组」直接走 reports_per_line，与取件回调同源，两边口径不会走偏。
fn group_plan_of(items: &[ItemSpec], staging: &Path, cached: &[bool]) -> Vec<(String, u32)> {
    let mut plan: Vec<(String, u32)> = Vec::new();
    for (it, c) in items.iter().zip(cached) {
        if reports_per_line(matches!(&it.fetch, Fetch::Url(_)), *c, 0) {
            continue;
        }
        let label = fetch_group_label(&it.dest, staging);
        match plan.iter_mut().find(|(l, _)| *l == label) {
            Some(g) => g.1 += 1,
            None => plan.push((label, 1)),
        }
    }
    plan
}

/// 落位路径 → 分组名：staging 下第一层目录就是一个取件目录（mods/ 说「模组」），
/// 根下散件只有加载器 jar
fn fetch_group_label(dest: &Path, staging: &Path) -> String {
    let rel = dest.strip_prefix(staging).unwrap_or(dest);
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return "加载器".to_string();
    };
    if comps.next().is_none() {
        return "加载器".to_string();
    }
    let name = first.as_os_str().to_string_lossy().to_lowercase();
    if name == "mods" {
        "模组".to_string()
    } else {
        name
    }
}

fn apply_fetch_counts(
    t: &mut ConversionTask,
    done: usize,
    total: usize,
    net: &std::sync::atomic::AtomicU32,
    bytes: &std::sync::atomic::AtomicU64,
) {
    t.downloaded = Some(done as u32);
    t.total = Some(total as u32);
    t.net_done = Some(net.load(Ordering::Relaxed));
    t.done_bytes = Some(bytes.load(Ordering::Relaxed));
    t.progress = 30 + (52 * done as u32).checked_div(total as u32).unwrap_or(0).max(1);
}

/// 兜底刷出仍未出过日志的分组：整轮取件成功结束后走一次，账本对不上时不至于静默
fn flush_groups(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    groups: &Arc<Mutex<FetchGroups>>,
    net: &Arc<std::sync::atomic::AtomicU32>,
    bytes: &Arc<std::sync::atomic::AtomicU64>,
) {
    for g in groups.lock().unwrap().pending() {
        let (net_u, bytes_u) = (net.clone(), bytes.clone());
        log_update(
            app,
            state,
            id,
            PipelineStage::Downloader,
            LogLevel::Info,
            &group_line(&g),
            move |t| apply_fetch_counts(t, g.done, g.total, &net_u, &bytes_u),
        );
    }
}

/* ---------------- 当前动作（实时条，不进日志环） ---------------- */

/// 实时条最小重绘间隔：进度事件一次要全量刷 UI，逐块发会把界面烧穿
/// （日志量级 = 前端性能预算，同理适用于 activity）
const ACTIVITY_WINDOW: std::time::Duration = std::time::Duration::from_millis(150);

/// 联网传输的实时账本：reqwest 每个响应块回调一次，比日志密两个数量级，
/// 这里只累加字节，到窗口点才产出一条 ActivityInfo 随进度事件下发。
/// 锁顺序固定为「先本账本、后任务表」，且任务表回调里不得回头锁账本（防互锁）。
#[derive(Default)]
struct NetActivity {
    /// dest → (文件名, 已收, 该响应 Content-Length)：只装进行中的条目，收完即结算移出
    inflight: HashMap<PathBuf, (String, u64, u64)>,
    /// 已完成联网条目的字节
    settled: u64,
    /// 已完成条目里 Content-Length 已知的那部分（计划没给量时用它兜底）
    settled_known: u64,
    /// 计划口径：需联网总字节 / 总条数（建计划时算好，整轮不变）
    planned_bytes: u64,
    items_total: u32,
    subject: String,
    attempt: u32,
    last_emit: Option<std::time::Instant>,
    last_bytes: u64,
    rate: f64,
}

impl NetActivity {
    fn new(planned_bytes: u64, items_total: u32) -> Self {
        Self { planned_bytes, items_total, ..Default::default() }
    }

    fn done(&self) -> u64 {
        self.settled + self.inflight.values().map(|(_, d, _)| *d).sum::<u64>()
    }

    /// 分母优先取计划量（含还没开跑的条目），计划没给数才退回 Content-Length 累加
    fn total(&self) -> u64 {
        if self.planned_bytes > 0 {
            return self.planned_bytes;
        }
        self.settled_known + self.inflight.values().map(|(_, _, t)| *t).sum::<u64>()
    }

    fn snapshot(&self, items_done: u32) -> ActivityInfo {
        ActivityInfo {
            kind: ActivityKind::Net,
            subject: self.subject.clone(),
            done_bytes: self.done(),
            total_bytes: self.total(),
            items_done,
            items_total: self.items_total,
            rate_bps: self.rate,
            attempt: self.attempt.max(1),
        }
    }

    /// 记一个响应块；返回 Some(info) 表示到出图点了。速率按两次出图之间的字节差算，
    /// 再做一点平滑，免得采样窗口边界上数字乱跳
    fn record(&mut self, p: &TransferProgress, items_done: u32) -> Option<ActivityInfo> {
        let e = self
            .inflight
            .entry(p.key.clone())
            .or_insert_with(|| (p.file_name.clone(), 0, 0));
        e.1 = p.done;
        e.2 = p.total;
        self.subject = p.file_name.clone();
        self.attempt = p.attempt;
        let now = std::time::Instant::now();
        let due = self
            .last_emit
            .map(|t| now.duration_since(t) >= ACTIVITY_WINDOW)
            .unwrap_or(true);
        let done = self.done();
        if let Some(prev) = self.last_emit {
            let dt = now.duration_since(prev).as_secs_f64();
            if dt > 0.0 {
                let inst = done.saturating_sub(self.last_bytes) as f64 / dt;
                self.rate = if self.rate > 0.0 { self.rate * 0.7 + inst * 0.3 } else { inst };
            }
        }
        if !due {
            return None;
        }
        self.last_emit = Some(now);
        self.last_bytes = done;
        Some(self.snapshot(items_done))
    }

    /// 一条收完：从在飞集合结算。集合里没有的说明是缓存命中（零传输），不计
    fn settle(&mut self, key: &Path, bytes: u64) {
        if let Some((_, _, known)) = self.inflight.remove(key) {
            self.settled += bytes;
            self.settled_known += known;
        }
    }
}

/// 打包实时账本：ZipWriter 每写一个文件回调一次，口径与联网侧一致
#[derive(Default)]
struct ZipActivity {
    files_total: u32,
    files_done: u32,
    bytes_total: u64,
    bytes_done: u64,
    subject: String,
    started: Option<std::time::Instant>,
    last_emit: Option<std::time::Instant>,
}

impl ZipActivity {
    fn plan(&mut self, files: usize, bytes: u64) {
        self.files_total = files as u32;
        self.bytes_total = bytes;
        self.started = Some(std::time::Instant::now());
    }

    fn snapshot(&self) -> ActivityInfo {
        let secs = self.started.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
        ActivityInfo {
            kind: ActivityKind::Zip,
            subject: self.subject.clone(),
            done_bytes: self.bytes_done,
            total_bytes: self.bytes_total,
            items_done: self.files_done,
            items_total: self.files_total,
            // 打包侧看的是平均吞吐：单文件之间的瞬时差没有意义
            rate_bps: if secs > 0.0 { self.bytes_done as f64 / secs } else { 0.0 },
            attempt: 1,
        }
    }

    fn file(&mut self, group: &str, bytes: u64) -> Option<ActivityInfo> {
        self.files_done += 1;
        self.bytes_done += bytes;
        self.subject = group.to_string();
        let now = std::time::Instant::now();
        let due = self
            .last_emit
            .map(|t| now.duration_since(t) >= ACTIVITY_WINDOW)
            .unwrap_or(true);
        if !due {
            return None;
        }
        self.last_emit = Some(now);
        Some(self.snapshot())
    }
}

/// 人读体积（日志文案用；1MB = 1000KB 口径，与前端 formatSize 一致）
fn fmt_size(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.2} GB", b / KB / KB / KB)
    } else if b >= KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 打包进度：84 → 99 按已写字节铺开。旧写法全程钉在 90 再跳 100，几百个文件的
/// 压缩时间里界面一动不动，看起来就像卡死
fn build_progress(done: u64, total: u64) -> u32 {
    let step = done.saturating_mul(15).checked_div(total.max(1)).unwrap_or(0).min(15);
    84 + step as u32
}

/// 列表简述：最多 8 项，超出补「等 N 个」
fn brief_list(items: &[String]) -> String {
    if items.len() <= 8 {
        return items.join("、");
    }
    format!("{}…等 {} 个", items[..8].join("、"), items.len())
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
            t.activity = None;
            push_line(
                t,
                log_line(
                    error.stage,
                    LogLevel::Error,
                    &format!("{} · {}", error.title, error.detail),
                ),
            );
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
        builder::build(&input, &mut |ev| {
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
        })
    })
    .await;

    let built = match build_result {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 分组名口径：staging 第一层目录即一条日志的归属，mods/ 说「模组」，根下散件说「加载器」
    #[test]
    fn fetch_group_label_maps_dest_to_folder() {
        let staging = PathBuf::from("/cache/tasks/t1/staging");
        assert_eq!(fetch_group_label(&staging.join("mods").join("a.jar"), &staging), "模组");
        assert_eq!(fetch_group_label(&staging.join("config").join("a.toml"), &staging), "config");
        assert_eq!(
            fetch_group_label(&staging.join("kubejs").join("client").join("x.js"), &staging),
            "kubejs"
        );
        assert_eq!(fetch_group_label(&staging.join("fabric-server-launch.jar"), &staging), "加载器");
        // 大小写不同的 Mods/ 归同一组
        assert_eq!(fetch_group_label(&staging.join("Mods").join("a.jar"), &staging), "模组");
    }

    /// 只有「真联网」的条目逐条报：命中缓存的联网坐标、包内、本地条目一律归入目录账本
    #[test]
    fn only_real_network_reports_per_line() {
        assert!(reports_per_line(true, false, 0), "未命中缓存的联网条目逐条报");
        assert!(!reports_per_line(true, true, 0), "联网坐标命中缓存 → 走复制，入目录");
        assert!(!reports_per_line(false, false, 0), "包内/本地 → 入目录");
        assert!(reports_per_line(false, false, 2), "重试过就该看得见");
    }

    fn spec_at(fetch: Fetch, dest: PathBuf) -> ItemSpec {
        ItemSpec {
            file_name: dest
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            sha1: None,
            dest,
            size_bytes: 10,
            fetch,
        }
    }

    /// 计划与回调同源：按 group_plan_of 的预设数走一遍回调路由，每个目录恰好出一条
    #[test]
    fn group_plan_matches_runtime_routing() {
        let staging = PathBuf::from("/cache/tasks/t1/staging");
        let archive = PathBuf::from("/pack.mrpack");
        let items = vec![
            spec_at(Fetch::ZipEntry { archive: archive.clone(), entry: "override/config/a.cfg".into() }, staging.join("config").join("a.cfg")),
            spec_at(Fetch::ZipEntry { archive: archive.clone(), entry: "override/config/b.cfg".into() }, staging.join("config").join("b.cfg")),
            spec_at(Fetch::Local(PathBuf::from("/local/a.jar")), staging.join("mods").join("a.jar")),
            spec_at(Fetch::Url("https://m/1.jar".into()), staging.join("mods").join("1.jar")),
            spec_at(Fetch::Url("https://m/2.jar".into()), staging.join("mods").join("2.jar")),
            spec_at(Fetch::Url("https://dl/fabric.jar".into()), staging.join("fabric-server-launch.jar")),
        ];
        // 联网坐标里 mods/2.jar 与根下的服务端 jar 命中缓存 → 走复制，归入各自目录
        let cached = [false, false, false, false, true, true];
        let plan = group_plan_of(&items, &staging, &cached);
        assert_eq!(
            plan,
            vec![("config".to_string(), 2u32), ("模组".to_string(), 2u32), ("加载器".to_string(), 1u32)]
        );

        let mut g = FetchGroups::new(plan);
        let mut per_line = 0;
        let mut group_lines = 0;
        for (i, it) in items.iter().enumerate() {
            let network = matches!(&it.fetch, Fetch::Url(_));
            if reports_per_line(network, cached[i], 0) {
                per_line += 1;
                continue;
            }
            let label = fetch_group_label(&it.dest, &staging);
            if g.record(&label, 10, i + 1, items.len()).is_some() {
                group_lines += 1;
            }
        }
        assert_eq!(per_line, 1, "只有未命中缓存的那个联网 jar 逐条报");
        assert_eq!(group_lines, 3, "三个目录各一条");
        assert!(g.pending().is_empty(), "计划与回调对得上时不该有兜底残留");
    }

    /// 一个目录只出一条：取满预设数才发，过程中只走心跳
    #[test]
    fn groups_emit_one_line_per_folder_when_full() {
        let mut g = FetchGroups::new(vec![("模组".into(), 2), ("config".into(), 3)]);
        assert!(g.record("模组", 10, 1, 5).is_none());
        let snap = g.record("模组", 20, 2, 5).expect("取满 2 个应出一条");
        assert_eq!(snap.files, 2);
        assert_eq!(snap.bytes, 30);
        assert_eq!(group_line(&snap), "取件 · 模组 · 2 个 · 30 B");
        // 同目录再来一条（预设外的迟到项）不应重复出
        assert!(g.record("模组", 99, 3, 5).is_none());
        // 未取满的目录不出
        assert!(g.record("config", 1, 4, 5).is_none());
        assert!(g.record("config", 1, 5, 5).is_none());
        assert!(g.record("config", 1, 6, 5).is_some());
        // 兜底只补没出过的，且已出过的不再重复
        let left = g.pending();
        assert!(left.iter().all(|r| r.label != "模组"));

        let mut g2 = FetchGroups::new(vec![("kubejs".into(), 100)]);
        assert!(g2.record("kubejs", 5, 1, 100).is_none());
        let left = g2.pending();
        assert_eq!(left.len(), 1, "未取满的目录在整轮结束后要兜底补一条");
        assert_eq!(left[0].files, 1);
        assert!(g2.pending().is_empty(), "兜底刷出后不应重复");
    }

    /// 心跳：首次立即可发（进度条别等到取满才动），随后受间隔约束；
    /// 窗口起点记在账本上，取条目/发日志都不该把它清空
    #[test]
    fn group_beat_throttles_progress_only_updates() {
        let mut g = FetchGroups::new(vec![("config".into(), 9999)]);
        assert!(g.beat_due(), "首次应立即可发，否则进度条会长时间不动");
        assert!(!g.beat_due(), "间隔未到不应再发");
        g.last_beat = Some(std::time::Instant::now() - BEAT_INTERVAL * 2);
        assert!(g.beat_due(), "间隔已过应再发");
    }

    fn tp(key: &str, done: u64, total: u64, attempt: u32) -> TransferProgress {
        TransferProgress {
            file_name: format!("{key}.jar"),
            key: PathBuf::from(key),
            done,
            total,
            attempt,
        }
    }

    /// 实时条：并发下载的字节合到一条、窗口未到不出图、结算不重复计数
    #[test]
    fn net_activity_aggregates_concurrent_files_and_throttles() {
        let mut a = NetActivity::new(300, 2);
        assert!(a.record(&tp("/d/a", 50, 150, 1), 0).is_some(), "首次应立即出图，否则条不动");
        assert!(a.record(&tp("/d/b", 30, 150, 1), 0).is_none(), "窗口未到不应再出图");
        assert_eq!(a.done(), 80, "在飞两条的字节应合并计数");
        a.settle(&PathBuf::from("/d/a"), 150);
        assert_eq!(a.done(), 180, "结算按实际字节计，不与在飞量重复");
        assert_eq!(a.total(), 300, "计划有量就用计划量当分母");
        a.last_emit = Some(std::time::Instant::now() - ACTIVITY_WINDOW * 2);
        let info = a.record(&tp("/d/b", 150, 150, 2), 1).expect("窗口已过应再出图");
        assert_eq!(info.done_bytes, 300);
        assert_eq!(info.attempt, 2, "第几次重试要看得见");
        assert_eq!(info.items_total, 2);
        assert!(info.rate_bps > 0.0, "速率应算出来");
    }

    /// 计划没给字节（例如 maven 坐标无伴生大小）：分母退回 Content-Length 累加
    #[test]
    fn net_activity_falls_back_to_content_length() {
        let mut a = NetActivity::new(0, 2);
        a.record(&tp("/d/a", 10, 100, 1), 0).unwrap();
        a.settle(&PathBuf::from("/d/a"), 10);
        a.last_emit = Some(std::time::Instant::now() - ACTIVITY_WINDOW * 2);
        let info = a.record(&tp("/d/b", 5, 50, 1), 1).expect("窗口已过应出图");
        assert_eq!(info.total_bytes, 150, "已结算 + 在飞的 Content-Length");
        assert_eq!(info.done_bytes, 15);
    }

    /// 打包侧同样按窗口出图，subject 跟随当前顶层目录
    #[test]
    fn zip_activity_throttles_per_file() {
        let mut z = ZipActivity::default();
        z.plan(3, 300);
        assert!(z.file("模组", 100).is_some(), "首个文件应立即出图");
        assert!(z.file("模组", 100).is_none(), "窗口未到不应再出图");
        z.last_emit = Some(std::time::Instant::now() - ACTIVITY_WINDOW * 2);
        let info = z.file("根文件", 100).expect("窗口已过应出图");
        assert_eq!((info.done_bytes, info.total_bytes), (300, 300));
        assert_eq!((info.items_done, info.items_total), (3, 3));
        assert_eq!(info.subject, "根文件");
        assert_eq!(info.kind, ActivityKind::Zip);
    }

    /// 打包进度必须在 84→99 之间真实铺开（旧写法全程钉 90，看着像卡死）
    #[test]
    fn build_progress_spreads_over_the_zip_stage() {
        assert_eq!(build_progress(0, 1000), 84);
        assert_eq!(build_progress(500, 1000), 91);
        assert_eq!(build_progress(1000, 1000), 99, "打包阶段不满 100，成功收尾才给 100");
        assert_eq!(build_progress(0, 0), 84, "总量为 0 不能崩");
    }

    /// 日志环：超限只留最近 MAX_LOG_LINES 条，头部恰好一条截断说明
    #[test]
    fn log_ring_trims_to_recent_lines() {
        let mut logs: Vec<TaskLogLine> = Vec::new();
        for i in 0..(MAX_LOG_LINES + 250) {
            logs.push(log_line(
                PipelineStage::Downloader,
                LogLevel::Info,
                &format!("取件 · config · {i} 个"),
            ));
            trim_logs(&mut logs);
            assert!(logs.len() <= MAX_LOG_LINES);
        }
        assert_eq!(logs.len(), MAX_LOG_LINES);
        assert_eq!(
            logs.iter().filter(|l| l.message.starts_with("（日志过长")).count(),
            1,
            "截断说明不应重复堆积"
        );
        assert!(logs[0].message.starts_with("（日志过长"));
        assert_eq!(
            logs.last().unwrap().message,
            format!("取件 · config · {} 个", MAX_LOG_LINES + 249)
        );
    }

    #[test]
    fn unique_output_names_avoid_overwrite() {
        let mut used = HashSet::new();
        assert_eq!(unique_mod_name(&mut used, "a.jar", "m1"), "a.jar");
        assert_eq!(unique_mod_name(&mut used, "a.jar", "m2"), "a-m2.jar");
        assert_eq!(unique_mod_name(&mut used, "a-m2.jar", "m3"), "a-m2-m3.jar");
    }
}

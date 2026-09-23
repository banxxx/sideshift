//! 单任务串行调度：抢独占槽位否则排队，跑完（成功/失败/取消）自动放行队首。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;

use crate::models::*;
use super::events::{log_line, push_line};
use super::persist::save_tasks;
use super::pipeline::spawn_pipeline;
use super::state::{AppState, Inner};
use super::{util::now_ms, CACHE_TASKS_DIR};

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

pub fn release_and_next(app: &AppHandle, state: &Arc<AppState>, done_id: &str) -> Option<String> {
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

/// 回收某个任务的暂存目录（cache/tasks/<id>），文件本体留在 cache/files 供跨任务复用
pub fn remove_task_staging(state: &AppState, id: &str) {
    let cache_dir = state.inner.lock().unwrap().settings.cache_dir.clone();
    let _ = std::fs::remove_dir_all(PathBuf::from(cache_dir).join(CACHE_TASKS_DIR).join(id));
}

/// 启动回收：上次进程被强杀时来不及清理的暂存目录（注册表里已无此任务即删）
pub fn sweep_task_staging(inner: &Inner) {
    let dir = PathBuf::from(&inner.settings.cache_dir).join(CACHE_TASKS_DIR);
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

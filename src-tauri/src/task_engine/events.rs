//! 进度事件与日志环：任务表更新、日志裁剪、事件广播、失败落档。

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::models::*;
use super::persist::save_tasks;
use super::state::AppState;
use super::util::{now_hms, now_ms};

pub const EVENT_PROGRESS: &str = "conversion://progress";
pub const EVENT_DONE: &str = "conversion://done";
/// 自动分类结果（离线层一次、在线层一次）：与任务流水线无关，只刷方案表
pub const EVENT_CLASSIFIED: &str = "plan://classified";

/// 单任务日志上限：保留目录命中 kubejs/资源包这类目录时条目数以千计，日志不设上限会把
/// tasks.json 撑到 MB 级、并让每次进度事件都全量搬运日志给前端 —— 实测 7340 行直接把界面卡死。
/// 超限后只留最近这些条（含一条截断标记），复制与存档口径一致。
const MAX_LOG_LINES: usize = 600;

/// 超限时裁到最近 MAX_LOG_LINES 条，并在头部放一条截断说明（旧头已被裁掉，所以永远只有一条）
pub fn trim_logs(logs: &mut Vec<TaskLogLine>) {
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

pub fn log_line(stage: PipelineStage, level: LogLevel, message: &str) -> TaskLogLine {
    TaskLogLine {
        time: now_hms(),
        stage,
        message: message.to_string(),
        level,
    }
}

pub fn push_line(t: &mut ConversionTask, line: TaskLogLine) {
    t.logs.push(line);
    trim_logs(&mut t.logs);
}

pub fn emit_progress(app: &AppHandle, t: &ConversionTask) {
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
pub fn update(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    f: impl FnOnce(&mut ConversionTask),
    emit: bool,
) {
    let mut inner = state.inner.lock().unwrap();
    if let Some(t) = inner.tasks.get_mut(id) {
        f(t);
        if emit {
            emit_progress(app, t);
        }
    }
}

pub fn push_log(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    stage: PipelineStage,
    level: LogLevel,
    msg: &str,
) {
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
pub fn log_update(
    app: &AppHandle,
    state: &Arc<AppState>,
    id: &str,
    stage: PipelineStage,
    level: LogLevel,
    msg: &str,
    f: impl FnOnce(&mut ConversionTask),
) {
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

/// 任务转失败：写状态 + 一条 error 日志 + done 事件 + 落盘
pub fn fail(app: &AppHandle, state: &Arc<AppState>, id: &str, error: TaskError) {
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
    notify_done(app, id);
    save_tasks(app, &state.inner.lock().unwrap());
}

/// 流水线终止信号（失败或成功收尾都要发）
pub fn notify_done(app: &AppHandle, id: &str) {
    let _ = app.emit(EVENT_DONE, HashMap::from([("taskId".to_string(), id.to_string())]));
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

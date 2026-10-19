//! 进度事件与日志环：任务表更新、日志裁剪、事件广播、失败落档。

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::l10n::Msg;
use crate::models::*;
use crate::msg;
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
/// 全量的那一遍不在这里，它按行写在 `{cache}\tasks\{id}\logs\` 下本轮那一份（`core::task_log`）。
/// 本机开服验证的输出流用的是**同一条预算**（`task_engine::verify`），所以这里对两边可见。
pub(crate) const MAX_LOG_LINES: usize = 600;

/// 「日志过长」那一句截断说明：转换任务日志与验证日志共用同一句、同一个数
pub(crate) fn truncation_msg() -> Msg {
    msg!("（日志过长，仅保留最近 {{count}} 条）", {"count": MAX_LOG_LINES - 1})
}

/// 超限时裁到最近 MAX_LOG_LINES 条，并在头部放一条截断说明（旧头已被裁掉，所以永远只有一条）
pub fn trim_logs(logs: &mut Vec<TaskLogLine>) {
    if logs.len() <= MAX_LOG_LINES {
        return;
    }
    let stage = logs.last().map(|l| l.stage).unwrap_or(PipelineStage::Parser);
    logs.drain(..logs.len() - (MAX_LOG_LINES - 1));
    logs.insert(0, log_line(stage, LogLevel::Warn, truncation_msg()));
}

/// 一条日志。`msg` 是「模板 + 参数」（见 `crate::l10n`）：渲染出的中文整句进 `message`（存档、
/// 复制、旧快照兜底全用它），模板与参数另挂一份给界面查翻译目录——带文件名/数字的句子
/// 只有这样才能翻，光靠「中文整句 → 键」的精确表永远差那几个字。
pub fn log_line(stage: PipelineStage, level: LogLevel, msg: Msg) -> TaskLogLine {
    TaskLogLine {
        time: now_hms(),
        stage,
        message: msg.zh.clone(),
        message_msg: Some(msg),
        level,
    }
}

pub fn push_line(t: &mut ConversionTask, line: TaskLogLine) {
    // 每一行先落文件再进环：这一条是所有日志写点的汇流处（解析/下载/打包/失败/取消都走它），
    // 逐处补写迟早漏一处。文件不在（冷启动后的旧任务、目录建不出来）就只演环，转换照跑
    if let Some(path) = t.log_file.as_deref() {
        crate::core::task_log::append(path, &line);
    }
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
    msg: Msg,
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
    msg: Msg,
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
                    // 「标题 · 明细」两段成一句：标题是我们登记的固定句，明细优先用它自己那句
                    // 「模板 + 参数」（嵌进来当参数，界面连它一起翻）；没有模板的那几种（外部进程的
                    // 诊断串）原样带出，判据在前端归一化那层（见 `src/lib/log-normalize.ts` 的 `logText`）
                    msg!(
                        "{{title}} · {{detail}}",
                        {
                            "title": &error.title,
                            "detail": error
                                .detail_msg
                                .clone()
                                .unwrap_or_else(|| Msg::foreign(error.detail.clone()))
                                .arg()
                        }
                    ),
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
                // 测试数据不必登记模板，用「外部原文」那档构造即可
                Msg::foreign(format!("取件 · config · {i} 个")),
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

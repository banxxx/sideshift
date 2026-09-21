//! 本地持久化：全局设置（settings.json）与任务存档（tasks.json）。

use std::collections::HashMap;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::models::*;
use super::events::trim_logs;
use super::state::Inner;
use super::util::now_ms;

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
        }
        .with_native_dirs(),
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
pub fn load_tasks(app: &AppHandle, inner: &mut Inner) {
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

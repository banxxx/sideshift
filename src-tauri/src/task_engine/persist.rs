//! 本地持久化：全局设置（settings.json）与任务存档（tasks.json）。

use std::collections::HashMap;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::core::data_root;
use crate::models::*;
use super::events::trim_logs;
use super::state::Inner;
use super::util::now_ms;

const SETTINGS_FILE: &str = "settings.json";
/// 目录默认值迁移做过一次的凭据（空文件，与 settings.json 同目录）
const SETTINGS_MIGRATED: &str = "settings.migrated";
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

/// 应用自己的状态落在哪：便携包写在 exe 同级的 `data`（整包搬走=连设置一起搬走），
/// 安装版/开发模式走 Tauri 的 app_config_dir。收口成一个函数，是为了让
/// 「便携模式下一切跟着 exe 走」这条语义只有一个实现点，不会漏掉某个文件
fn config_dir(app: &AppHandle) -> Option<PathBuf> {
    match data_root::portable_root() {
        Some(dir) => Some(dir),
        None => app.path().app_config_dir().ok(),
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).map(|d| d.join(SETTINGS_FILE))
}

/// 还没迁移 → 返回标记文件路径；已迁移或拿不到配置目录 → None（宁可不迁，也不能反复顶掉用户的路径）
fn migration_mark(app: &AppHandle) -> Option<PathBuf> {
    let mark = config_dir(app)?.join(SETTINGS_MIGRATED);
    (!mark.exists()).then_some(mark)
}

pub fn load_settings(app: &AppHandle) -> AppSettings {
    let home = app
        .path()
        .home_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    // 数据根：便携包 → exe 同级 data；安装版 → 安装壳指定的根；都没有才预选非系统盘
    let config = config_dir(app).unwrap_or_else(|| home.clone());
    let defaults = AppSettings::defaults_in(&data_root::suggested_root(&home, &config));
    let saved = settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok());
    match saved {
        Some(mut s) => {
            // 空目录字段回落默认值（用户清空输入框的场景）
            if s.output_dir.is_empty() {
                s.output_dir = defaults.output_dir.clone();
            }
            if s.cache_dir.is_empty() {
                s.cache_dir = defaults.cache_dir.clone();
            }
            s = s.normalized();

            // 一次性迁移：老版本的默认目录挂在用户目录下，那份绝对路径固化在 settings.json 里，
            // 不改写的话本次「预选非系统盘」对升级用户等于没发生。旧默认由同一套布局现算，
            // 不抄字符串（见 unstick_legacy 的取舍说明）。
            // 「已经迁过」必须靠标记文件记住，不能拿字段值当凭据：用户后来手动把目录设回
            // `~\SideShift\output` 时，那串字符和旧默认完全相同，没标记就会每次启动顶掉他一次
            if let Some(mark) = migration_mark(app) {
                let legacy = AppSettings::defaults_in(&home.join(data_root::DIR_NAME)).normalized();
                let out = unstick_legacy(&s.output_dir, &legacy.output_dir, &defaults.output_dir);
                let cache = unstick_legacy(&s.cache_dir, &legacy.cache_dir, &defaults.cache_dir);
                let changed = out != s.output_dir || cache != s.cache_dir;
                s.output_dir = out;
                s.cache_dir = cache;
                // 落盘成功才立标记：写失败就不立，下次启动重算——半途而废的迁移最查不出来
                if !changed || save_settings(app, &s).is_ok() {
                    let _ = std::fs::write(mark, "");
                }
            }
            s
        }
        None => defaults,
    }
}

/// 写 settings.json：失败要报给调用方。静默失败等于「用户改完设置、重启后回退」，
/// 而他看到的是保存成功的界面——这种账没法查，所以宁可吵一句。
pub fn save_settings(app: &AppHandle, s: &AppSettings) -> Result<(), String> {
    let Some(p) = settings_path(app) else {
        return Err("找不到应用配置目录，设置未能保存".to_string());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("设置写入失败：{e}（{}）", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("设置序列化失败：{e}"))?;
    std::fs::write(&p, json).map_err(|e| format!("设置写入失败：{e}（{}）", p.display()))
}

/* ---------------- 任务存档 ---------------- */

fn tasks_path(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).map(|d| d.join(TASKS_FILE))
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
                code: None,
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

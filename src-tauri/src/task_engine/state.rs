//! 全局状态：内存任务注册表 + 回收站 + 已解析包缓存 + 设置 + 转换模板 + 独占运行槽位。

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::AppHandle;

use crate::core::env;
use crate::core::parser::ParsedPack;
use crate::models::*;
use super::persist::{load_settings, load_templates, load_tasks};
use super::schedule::sweep_task_staging;
use super::trash::Trashed;

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
    /// 回收站：本次会话删掉的任务。只活在内存（不进 tasks.json），所以关应用即清空
    pub trash: HashMap<String, Trashed>,
    pub settings: AppSettings,
    /// 转换模板表（有序：下标就是转换页那颗下拉的顺序）
    pub templates: Vec<ConversionTemplate>,
    /// 当前独占运行的任务 id——同一时间只允许一条转换在跑，其余排队
    pub current: Option<String>,
    /// 最近一次自动分类取到的端证据（包内条目路径 → 证据）；只属于 env_evidence_file 那个包
    pub env_evidence: env::EvidenceMap,
    /// env_evidence 归属的包名（换包即作废）
    pub env_evidence_file: Option<String>,
    /// 最近一次离线扫描的字节码结构事实（路径 → 事实）；与 env_evidence 同期写、同包作废
    pub env_code: env::CodeMap,
    /// 联网反查仍在后台跑的那个包名（跑完或换包即清）。classify_pack 靠它区分
    /// 「缓存命中、本轮已经结束」和「缓存命中、但在线层还在补」——后者还得继续挂「分类中」。
    pub env_online_file: Option<String>,
}

pub struct AppState {
    pub inner: Mutex<Inner>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Self {
        let mut inner = Inner {
            settings: load_settings(app),
            templates: load_templates(app),
            ..Default::default()
        };
        load_tasks(app, &mut inner);
        sweep_task_staging(&inner);
        Self {
            inner: Mutex::new(inner),
        }
    }
}

/// 任务是否还在跑（排队或运行中）。注意取消后调度器会立刻把下一条置为 Running，
/// 已取消的本任务用这个判断会误读成真，回调必须自持取消标志。
pub fn is_active(state: &Arc<AppState>, id: &str) -> bool {
    let inner = state.inner.lock().unwrap();
    matches!(
        inner.tasks.get(id),
        Some(t) if matches!(t.status, TaskStatus::Queued | TaskStatus::Running)
    )
}

/// 有没有还没跑完的转换（排队或运行中）。应用内更新用它挡一下：那条链的最后一句是退出进程，
/// 正跑到一半的任务存档会停在 `running`，下次启动就是一条永远不动的任务——所以这颗闸门不给强制档。
/// 与 `is_active` 分开写：那边按 id 问「这一条还在不在跑」，这边问「有没有任何一条还在跑」。
pub fn has_active_tasks(state: &Arc<AppState>) -> bool {
    let inner = state.inner.lock().unwrap();
    inner
        .tasks
        .values()
        .any(|t| matches!(t.status, TaskStatus::Queued | TaskStatus::Running))
}

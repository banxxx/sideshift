//! 回收站：删除是「软删除 + 会话级暂存」。
//!
//! 为什么记录只躺在内存：垃圾桶的语义就是「这次开着应用时删掉的」——用户要的是关应用即清空，
//! 所以这里刻意不写 tasks.json。进程退出后记录自然消失，而它占用的暂存目录由下次启动的
//! `sweep_task_staging`（注册表里已无此任务即删）收掉，不需要退出钩子：`std::process::exit`
//! 那类强杀路径本来也不给钩子机会，靠启动清扫才是稳定口径。
//!
//! 反过来说，撤回要求「暂存目录不能被删」——所以 `delete_task` 从原来的「立即递归删暂存」
//! 改成「搬进回收站、暂存原地保留」，真正丢弃发生在 `clear_trash`。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;

use crate::models::*;
use super::persist::save_tasks;
use super::state::Inner;
use super::util::now_ms;

/// 一次删除搬走的全部上下文：撤回要把「任务 + 方案 + 报告」原样放回
pub struct Trashed {
    pub task: ConversionTask,
    pub plan: Option<Vec<PlanMod>>,
    pub report: Option<ConversionReport>,
    pub deleted_at: i64,
}

/// 回收站行（弹窗只列要点，不把整份任务连日志一起搬运）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub task_id: String,
    pub pack_file_name: String,
    pub loader: LoaderKind,
    pub mc_version: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_size_bytes: Option<u64>,
    /// 删除时刻（epoch ms）：弹窗按它倒序，也用来显示「12 秒前删的」
    pub deleted_at: i64,
}

/// 删除 = 从注册表搬进回收站并落盘（存档里从此没有它），返回 false 表示没有这条任务。
/// 注意这里**不**动暂存目录：撤回要靠它，产物与下载缓存也都还在原地。
pub fn trash_task(app: &AppHandle, inner: &mut Inner, id: &str) -> bool {
    let Some(task) = inner.tasks.remove(id) else {
        return false;
    };
    inner.trash.insert(
        id.to_string(),
        Trashed {
            task,
            plan: inner.plans.remove(id),
            report: inner.reports.remove(id),
            deleted_at: now_ms(),
        },
    );
    inner.cancel.remove(id);
    save_tasks(app, inner);
    true
}

/// 撤回：原样放回注册表与存档。暂存目录没动过，所以不需要重建任何东西。
pub fn restore_task(app: &AppHandle, inner: &mut Inner, id: &str) -> Result<(), String> {
    let Some(trashed) = inner.trash.remove(id) else {
        return Err("回收站里已经没有这条任务（可能刚被清空）".to_string());
    };
    // 同 id 又在列表里 = 撤回会盖掉一条活着的任务，这种状态不该静默发生
    if inner.tasks.contains_key(id) {
        inner.trash.insert(id.to_string(), trashed);
        return Err("任务列表里已经有这条任务，撤回没有执行".to_string());
    }
    inner.tasks.insert(id.to_string(), trashed.task);
    if let Some(plan) = trashed.plan {
        inner.plans.insert(id.to_string(), plan);
    }
    if let Some(report) = trashed.report {
        inner.reports.insert(id.to_string(), report);
    }
    // 取消标志必须补回：撤回后用户可能直接点「重新转换」，缺了标志流水线拿到的
    // 是自己新建的那份 Arc，取消按钮就按不动（见 pipeline 的 unwrap_or_else 回落）
    inner
        .cancel
        .entry(id.to_string())
        .or_insert_with(|| Arc::new(AtomicBool::new(false)));
    save_tasks(app, inner);
    Ok(())
}

/// 清空第一步（持锁）：把整叠记录换出来，只留 id 给解锁后的递归删。
/// 必须真的「换空」——只读 keys 不摘走的话，`list_trash` 下一拍仍把这些行吐回来，
/// 前端对账后回收站看着一条没少（长按清空就白按了）。
/// 暂存目录的递归删要在解锁之后做：`remove_task_staging` 会再锁同一把非重入 Mutex，嵌套即自死锁。
/// 这里不落盘：回收站本来就不进 tasks.json，删掉的那些行在 `trash_task` 时就已经不在存档里了。
pub fn drain_trash(inner: &mut Inner) -> Vec<String> {
    let drained = std::mem::take(&mut inner.trash);
    drained.keys().cloned().collect()
}

/// 弹窗列表：最近删掉的排最前
pub fn entries(inner: &Inner) -> Vec<TrashEntry> {

    let mut v: Vec<TrashEntry> = inner
        .trash
        .values()
        .map(|t| {
            let task = &t.task;
            TrashEntry {
                task_id: task.id.clone(),
                pack_file_name: task.pack.file_name.clone(),
                loader: task.pack.loader,
                mc_version: task.options.mc_version.clone(),
                status: task.status,
                output_file_name: task.output_file_name.clone(),
                output_size_bytes: task.output_size_bytes,
                deleted_at: t.deleted_at,
            }
        })
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.deleted_at));
    v
}

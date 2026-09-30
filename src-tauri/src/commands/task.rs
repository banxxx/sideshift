use super::*;

/* ---------------- 任务生命周期 ---------------- */

#[tauri::command]
pub fn start_conversion(
    app: AppHandle,
    state: S<'_>,
    options: ConversionOptions,
    manifest: PackManifest,
    plan: Vec<PlanMod>,
) -> task_engine::StartResult {
    task_engine::create_task(&app, &state, options, manifest, plan)
}

/// 列表接口每条任务只带末尾这些行：进度事件到达时前端会全量重拉列表（Home 轨道取末 3 行、
/// 任务行取末 1 行），而单任务日志上限是 600 行——多条任务全量搬运就是 MB 级载荷。
/// 详情页与报告页走 getTask，仍是全量。
const LIST_LOG_TAIL: usize = 8;

#[tauri::command]
pub fn list_tasks(state: S<'_>) -> Vec<ConversionTask> {
    let mut v: Vec<ConversionTask> = lock(&state)
        .tasks
        .values()
        .cloned()
        .map(|mut t| {
            if t.logs.len() > LIST_LOG_TAIL {
                let cut = t.logs.len() - LIST_LOG_TAIL;
                t.logs.drain(..cut);
            }
            t
        })
        .collect();
    v.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    v
}

#[tauri::command]
pub fn get_task(state: S<'_>, id: String) -> Option<ConversionTask> {
    lock(&state).tasks.get(&id).cloned()
}

#[tauri::command]
pub fn cancel_task(app: AppHandle, state: S<'_>, id: String) {
    task_engine::cancel(&app, &state, &id);
}

#[tauri::command]
pub fn retry_task(
    app: AppHandle,
    state: S<'_>,
    id: String,
) -> Option<task_engine::StartResult> {
    // 原地重试：同一 id、同一方案，产物落到自己上一份（详见 task_engine::retry_task）
    task_engine::retry_task(&app, &state, &id)
}

/// 删除 = 搬进回收站（撤回要用），所以**不**在这里递归删暂存目录——那笔账挪到了 clear_trash。
/// 记录本身已从 tasks.json 消失，所以进程退出后回收站自然空了（暂存残留由启动清扫兜底）。
#[tauri::command]
pub async fn delete_task(app: AppHandle, state: S<'_>, id: String) -> Result<(), String> {
    let mut inner = lock(&state);
    // 运行中的行不允许直接删（先取消）
    if inner.current.as_deref() == Some(id.as_str()) {
        return Ok(());
    }
    if task_engine::trash_task(&app, &mut inner, &id) {
        Ok(())
    } else {
        // 报出来而不是静默成就：走到这里说明前端把一条已经不在列表里的行又删了一次，
        // 那是状态机对不上（撤回/轮询竞态），不该被「反正结果一样」掩盖
        Err("这条任务已经不在列表里（可能刚被撤回或重复删除）".to_string())
    }
}

/// 回收站列表：只回弹窗要用的要点，不搬整份任务（日志能到几百行）
#[tauri::command]
pub fn list_trash(state: S<'_>) -> Vec<task_engine::TrashEntry> {
    task_engine::trash_entries(&lock(&state))
}

/// 撤回一条删除：任务连同方案与报告原样回到列表（暂存目录与产物一直没动）
#[tauri::command]
pub fn restore_task(app: AppHandle, state: S<'_>, id: String) -> Result<(), String> {
    let mut inner = lock(&state);
    task_engine::restore_task(&app, &mut inner, &id)
}

/// 清空回收站：这时才真正丢弃暂存目录。递归删除必须在解锁之后做——
/// remove_task_staging 会再锁同一把非重入 Mutex，嵌套即自死锁。
#[tauri::command]
pub async fn clear_trash(state: S<'_>) -> Result<usize, String> {
    let ids = {
        let mut inner = lock(&state);
        task_engine::drain_trash(&mut inner)
    };
    let n = ids.len();
    for id in ids {
        task_engine::remove_task_staging(&state, &id);
    }
    Ok(n)
}

#[tauri::command]
pub fn get_report(state: S<'_>, task_id: String) -> Option<ConversionReport> {
    lock(&state).reports.get(&task_id).cloned()
}

/// 某个任务创建时确认过的方案快照（报告页展开真实剔除/保留/新增清单）。
/// 不能用 `get_plan`：那个返回的是「最近一次解析的包」，用户换个包再看旧报告就会张冠李戴。
#[tauri::command]
pub fn get_task_plan(state: S<'_>, task_id: String) -> Vec<PlanMod> {
    lock(&state).plans.get(&task_id).cloned().unwrap_or_default()
}


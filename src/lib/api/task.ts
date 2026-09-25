/** 任务生命周期（创建 / 查询 / 取消 / 重试 / 删除 / 回收站 / 报告 / 进度事件） */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    ConversionOptions,
    ConversionReport,
    ConversionTask,
    PackManifest,
    PlanMod,
    ProgressEvent,
    StartResult,
    TrashEntry,
} from "@/lib/types";
import { EVENTS } from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 创建转换任务（Rust: start_conversion(options, manifest, plan) -> StartResult）；
 *  plan 为前端确认过的最终方案；同一时间只跑一条，已有任务在跑时新任务排队 */
export async function startConversion(
    options: ConversionOptions,
    pack: PackManifest,
    plan: PlanMod[]
): Promise<StartResult> {
    if (!isTauri) return mock.mockStartTask(options, pack);
    return invokeOrMock(
        "start_conversion",
        { options, manifest: pack, plan },
        () => mock.mockStartTask(options, pack)
    );
}

/**
 * 最近一次 `listTasks()` 的结论（模块级，跨页面挂载存续）。
 *
 * 侧栏换页会把整页重挂载，而这份数据后端就在内存里，重读一遍必是同一份 ⇒ 页面却先画一帧
 * 「正在读取任务…」再换掉它：那一换走的是 `Swap`（popLayout），退场的占位层被抽离文档流、
 * **直接叠在新列表上面**，加上整页自己在淡入上浮，看着就是两层不同数据的页面糊在一起。
 * 首帧拿它当初值就没这一拍了；真正的冷启动（还没读过）照旧走占位。
 */
let lastTaskList: ConversionTask[] | null = null;

/** 上一趟读数；null = 这个进程还没成功读过一次（此时「没有任务」还不能宣布） */
export function peekTasks(): ConversionTask[] | null {
    return lastTaskList;
}

/** 任务列表（Rust: list_tasks） */
export async function listTasks(): Promise<ConversionTask[]> {
    // 只在成功那一路写缓存：读失败留下上一次的值，页面顶一帧旧数据后由 1s 轮询换回来，
    // 比把「刚刚还有三条任务」当场抹成空列表好
    const list = isTauri
        ? await invokeOrMock<ConversionTask[]>("list_tasks", undefined, () => mock.mockListTasks())
        : await mock.mockListTasks();
    lastTaskList = list;
    return list;
}

/** 单任务（Rust: get_task(id)） */
export async function getTask(id: string): Promise<ConversionTask | undefined> {
    if (!isTauri) return mock.mockGetTask(id);
    return invokeOrMock("get_task", { id }, () =>
        mock.mockGetTask(id)
    ).then((t) => t ?? undefined);
}

/** 取消任务（Rust: cancel_task(id)） */
export async function cancelTask(id: string): Promise<void> {
    if (!isTauri) return mock.mockCancelTask(id);
    return invokeOrMock("cancel_task", { id }, () => mock.mockCancelTask(id));
}

/** 重试任务（Rust: retry_task(id) -> StartResult | null，语义同 start_conversion） */
export async function retryTask(id: string): Promise<StartResult | undefined> {
    if (!isTauri) return mock.mockRetryTask(id);
    return invokeOrMock<StartResult | undefined>(
        "retry_task",
        { id },
        () => mock.mockRetryTask(id)
    );
}

/** 删除任务记录（Rust: delete_task(id)）：搬进回收站，本次会话内可撤回 */
export async function deleteTask(id: string): Promise<void> {
    if (!isTauri) return mock.mockDeleteTask(id);
    return invokeOrMock("delete_task", { id }, () => mock.mockDeleteTask(id));
}

/** 回收站列表（Rust: list_trash）：按删除时刻倒序，关应用即空 */
export async function listTrash(): Promise<TrashEntry[]> {
    if (!isTauri) return mock.mockListTrash();
    return invokeOrMock("list_trash", undefined, () => mock.mockListTrash());
}

/** 撤回删除（Rust: restore_task(id)）：任务原样回到列表，暂存没动过 */
export async function restoreTask(id: string): Promise<void> {
    if (!isTauri) return mock.mockRestoreTask(id);
    return invokeOrMock("restore_task", { id }, () => mock.mockRestoreTask(id));
}

/** 清空回收站（Rust: clear_trash -> 条数）：这一步才真正丢弃暂存目录 */
export async function clearTrash(): Promise<number> {
    if (!isTauri) return mock.mockClearTrash();
    return invokeOrMock("clear_trash", undefined, () => mock.mockClearTrash());
}

/** 转换报告（Rust: get_report(taskId)） */
export async function getReport(taskId: string): Promise<ConversionReport | undefined> {
    if (!isTauri) return mock.mockReport(taskId);
    return invokeOrMock("get_report", { taskId }, () =>
        mock.mockReport(taskId)
    ).then((r) => r ?? undefined);
}

/** 任务创建时确认过的方案快照（Rust: get_task_plan(taskId)）：报告页展开真实清单用 */
export async function getTaskPlan(taskId: string): Promise<PlanMod[]> {
    if (!isTauri) return mock.mockGetTaskPlan(taskId);
    return invokeOrMock("get_task_plan", { taskId }, () =>
        mock.mockGetTaskPlan(taskId)
    );
}

/** 订阅流水线进度事件（浏览器 mock 模式无事件流，返回空取消函数） */
export function onProgress(cb: (e: ProgressEvent) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<ProgressEvent>(EVENTS.progress, (ev) => cb(ev.payload));
}

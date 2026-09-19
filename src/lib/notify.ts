/**
 * 全局提示条数据源：侧栏底部（版本号/主题行上方的预留提示区）的唯一写入口。
 *
 * 轻量、无需交互的结果反馈统一走 notify()（如「已更换模组构建版本」），
 * 后续其他类型提示（更新可用、后台任务完成等）也从这里挂载，不再各造局部 toast。
 * 消费方为 Sidebar 内嵌的 <NotificationStack/>，按 kind 映射图标与语义色。
 */
import { useSyncExternalStore } from "react";

export type NoticeKind = "info" | "success" | "warn" | "error";

export interface Notice {
    id: number;
    kind: NoticeKind;
    text: string;
}

/** 各 kind 停留时长（ms）：错误类读起来更长，给足确认时间 */
const DURATIONS: Record<NoticeKind, number> = {
    info: 3200,
    success: 3200,
    warn: 4500,
    error: 6000,
};

/** 同屏上限：超出时最早一条让位，提示区不挤占侧栏导航 */
const MAX_VISIBLE = 3;

let notices: Notice[] = [];
let seq = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
const listeners = new Set<() => void>();

function emit() {
    listeners.forEach((l) => l());
}

function remove(id: number) {
    const t = timers.get(id);
    if (t) {
        clearTimeout(t);
        timers.delete(id);
    }
    notices = notices.filter((n) => n.id !== id);
    emit();
}

/** 推入一条提示；返回 id，可提前 dismissNotice。超上限时立即挤掉最早一条 */
export function notify(text: string, kind: NoticeKind = "info"): number {
    const id = ++seq;
    notices = [...notices.slice(-(MAX_VISIBLE - 1)), { id, kind, text }];
    if (notices.length > MAX_VISIBLE) remove(notices[0].id);
    timers.set(id, setTimeout(() => remove(id), DURATIONS[kind]));
    emit();
    return id;
}

export function dismissNotice(id: number): void {
    remove(id);
}

export function useNotices(): Notice[] {
    return useSyncExternalStore(
        (cb) => {
            listeners.add(cb);
            return () => void listeners.delete(cb);
        },
        () => notices
    );
}

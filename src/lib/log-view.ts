/**
 * 日志展示口径：剪贴板文本 + 定高框的跟随滚动
 *
 * Home 轨道小窗与任务详情页共用同一份日志渲染约定，
 * 复制出的文本也必须在两处一致（便于用户直接贴出来排查）。
 */
import { useEffect, type RefObject } from "react";

export interface ClipLog {
    time?: string;
    stage?: string;
    message: string;
}

/** 一行一条：`14:02:11 [parser] 读取清单 …`；头部用于带上线程/任务上下文 */
export function logsToClipText(logs: ClipLog[], header?: string): string {
    const lines = logs.map((l) =>
        [l.time, l.stage ? `[${l.stage}]` : "", l.message].filter(Boolean).join(" ")
    );
    return header ? `${header}\n${lines.join("\n")}` : lines.join("\n");
}

/**
 * 定高日志框的贴底跟随：新日志到达时若已在底部附近就滚到底；
 * 用户上滑查看历史时不动滚动位置（否则每次事件都会把视野抢走）。
 */
export function useLogFollow(ref: RefObject<HTMLElement | null>, count: number): void {
    useEffect(() => {
        const el = ref.current;
        if (!el) return;
        if (el.scrollHeight - el.scrollTop - el.clientHeight < 48) {
            el.scrollTop = el.scrollHeight;
        }
    }, [count, ref]);
}

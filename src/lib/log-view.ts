/**
 * 日志展示口径：剪贴板文本 + 定高框跟随滚动。
 * Home 轨道小窗与任务详情页共用；复制出的文本必须在两处一致（便于直接贴出来排查）。
 */
import { useLayoutEffect, useRef, type RefObject } from "react";

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
 * 定高日志框的贴底跟随（两处日志框共用：任务详情页、Home 轨道小窗）。
 *
 * 四条口径各自对应一种失灵方式，别退回旧写法：
 *  - **"有没有新行"认的是「行数 + 末行文本」这一帧的签名**，不是数组长度、也不是内容总高：
 *    后端把日志裁成固定尾数（详情页 600 行 `events.rs::MAX_LOG_LINES`、Home 8 行
 *    `commands.rs::LIST_LOG_TAIL`，前端还另有一道 400 条渲染上限），跑满之后条数与框内总高
 *    都恒定不变，而新行其实一直在从头上顶旧行 —— 以"条数变了"或"变高了"为信号的跟随
 *    在长任务后期彻底失灵，Home 那侧则几乎从未生效过。
 *  - **跟不跟手由容器自己的 scroll 事件决定**（上滑松手、滑回底部再接上），而不是"新行进来之后
 *    再量一次距底"：一次轮询可以多塞进好几行，量到的距离当场就越过阈值，等于把"来了新日志"
 *    和"用户在读历史"混成同一个数。
 *  - 用 layout effect：effect 里改 `scrollTop` 得赶在这一帧画出来之前，
 *    否则用户先看到一条旧位置、再跳一下。
 *  - 两件事都放在「每次提交都跑」的 effect 里（依赖表是空的）：两处日志框挂在条件渲染的分支里
 *    （任务详情页在 `task` 到位前整棵树还不存在），带 `[ref]` 的 effect 在 ref 对象不变时不会
 *    再跑第二次，监听会永久挂空，用户上滑之后再也松不开手。
 */
export function useLogFollow(ref: RefObject<HTMLElement | null>): void {
    /** 用户是不是贴着底（初值真＝一打开就停在最新那条） */
    const atBottom = useRef(true);
    /** 上一次提交时框内内容的签名，用来认出"这一帧只是别的状态在重渲染" */
    const tail = useRef("");

    useLayoutEffect(() => {
        const el = ref.current;
        if (!el) return;
        const onScroll = () => {
            // 24px 容差 ≈ 一行多一点点：滚动条像素取整、末尾留白都不该把跟随关掉
            atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        };
        el.addEventListener("scroll", onScroll, { passive: true });

        const last = el.lastElementChild;
        const sig = `${el.childElementCount}|${last?.textContent ?? ""}`;
        const hasNewLine = sig !== tail.current;
        tail.current = sig;
        if (hasNewLine && atBottom.current) el.scrollTop = el.scrollHeight;

        return () => el.removeEventListener("scroll", onScroll);
    });
}

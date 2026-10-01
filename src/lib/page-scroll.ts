/**
 * 页面滚动容器 `main.page-scroll` 的两条口径都在这一处：
 *  1. **跨重挂的传送带**——只服务 `LocaleGate` 切语言的重挂：`stashScroll()` 在换 key 前接走位移
 *     （旧节点还在 DOM 才读得到），新节点登记时贴回，**读完即清**；只存一次待回填位移、不存每页位置。
 *  2. **换页签归顶**——页签换内容是一次换页，但滚动容器归 main 常驻、DOM 不会替我们清 ⇒ 显式归零。
 */
import { useLayoutEffect, useRef, type RefObject } from "react";

let node: HTMLElement | null = null;
let pending: number | null = null;

/** App 的滚动容器在 ref 上登记自己；`null` 是卸载，不能顺手把 `pending` 清掉——
 *  卸载（旧节点）与挂载（新节点）之间正是这次位移要跨过去的那道缝 */
export function setScrollNode(el: HTMLElement | null): void {
    node = el;
    if (!el || pending === null) return;
    el.scrollTop = pending;
    pending = null;
}

/** 把当前滚动位置接走，等下一个登记的节点贴回去 */
export function stashScroll(): void {
    pending = node ? node.scrollTop : null;
}

/**
 * `value` 变就把滚动归顶。给「一排页签换整块内容」那一类用：切到的那一档从自己的第一行开始，
 * 而不是继承上一档看到的行数。
 *
 * - 判据是「`value` 与上一次不同」，**不是数 effect 跑了几遍**（同公共 `Collapse` 的裁剪位）：
 *   StrictMode 会把挂载那次双跑，数遍数的写法会在第二跑里把 `stashScroll` 贴回来的位置抹掉。
 *   于是首帧天然相等 ⇒ 不归零，语言重挂那趟的位置也就保住了。
 * - `host` 传弹窗自己的滚动盒（那些不在 `main` 里，够不着页面容器）；不传就用页面容器。
 * - 用 `useLayoutEffect`：归顶和换内容的动效要在同一次提交里落定，晚一帧就是「先看见旧位置再跳」。
 */
export function useScrollResetOn<T>(value: T, host?: RefObject<HTMLElement | null>): void {
    const last = useRef(value);
    useLayoutEffect(() => {
        if (last.current === value) return;
        last.current = value;
        const el = host ? host.current : node;
        if (el) el.scrollTop = 0;
    }, [value, host]);
}

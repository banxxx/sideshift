/**
 * 页面滚动容器 `main.page-scroll` 的「跨重挂」传送带，只服务 `LocaleGate` 切语言的重挂。
 * `stashScroll()` 在换 key 前接走位移（旧节点还在 DOM 才读得到），新节点登记时贴回，**读完即清**。
 * 只存一次待回填位移、不存每页位置：普通换页也该从页顶开始。
 */
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

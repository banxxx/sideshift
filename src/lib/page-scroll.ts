/**
 * 页面滚动容器的一条「跨重挂」传送带。
 *
 * 只为一件事服务：`LocaleGate` 切语言时把视图整棵换 key 重挂（口径见那个文件），
 * 页面级滚动容器 `main.page-scroll` 也跟着没了，人刚从下拉里挑完语言，视野就被弹回页顶。
 * 这里不保存「每个页滚到哪」，只存**一次**待回填的位移：
 *  - `stashScroll()` 在换 key 之前调用（此刻旧节点还挂在 DOM 上，读得到）；
 *  - 新节点登记进来时贴回去，读完即清。
 *
 * 之所以是「读完即清」而不是留着：普通换页（点侧栏）也该从页顶开始，
 * 留着旧值会让下一次进这页莫名停在半路。
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

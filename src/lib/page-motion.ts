/**
 * 换页节拍与「页内错峰」的唯一出处。
 *
 * 换页走**缓动**而不是弹簧：一来弹簧的时长由它自己算，配不出「进 260 / 出 140」这一对定长；
 * 二来过冲与行程成正比（见 springs.ts 第 1 条），整页那几百 px 的行程一旦软到看得见弹就是几十 px 越线。
 * 要「弹」的只有任务卡那种峰值固定 px 的曲线（src/features/tasks/entry-curve.ts），它和这里的节拍是两套语义，别混用。
 *
 * 时序：旧页退 140ms → 新页淡入并上浮 24px、260ms 落定（App.tsx 用 mode="wait" 串起来，
 * 全程只有一层页面在屏上，所以滚动容器不必拆分、吸顶页头也不会两层叠印）。
 * 页内错峰由容器把节奏传给各块（PAGE_RISE → CARD_RISE），块与块之间 60ms。
 * 页签换页（TAB_SWEEP）是这套节拍的「小一号」版本：同一对进/出配比，行程与时长都缩一档。
 */
import type { Transition, Variants } from "motion/react";
import { RISE } from "./springs";

/** 页面容器入场 */
export const PAGE_IN: Transition = { duration: 0.26, ease: [0.2, 0.8, 0.2, 1] };
/** 页面容器退场：比入场短，指尖已经落到下一处，不该还等上一屏演完 */
export const PAGE_OUT: Transition = { duration: 0.14, ease: "easeOut" };

/** 页面入场容器：只管传节奏，自身不动（动的是页面整体，见 App.tsx） */
export const PAGE_RISE: Variants = {
    hidden: {},
    show: { transition: { staggerChildren: 0.06, delayChildren: 0.04 } },
};
/** 页内各块：依次上浮 14px（弹簧与帮助卡、方案行共用 RISE） */
export const CARD_RISE: Variants = {
    hidden: { opacity: 0, y: 14 },
    show: { opacity: 1, y: 0, transition: RISE },
};

/** 页签换页入场：比整页快一档——指尖还停在同一排页签上，不该等一屏 */
const TAB_IN: Transition = { duration: 0.2, ease: [0.2, 0.8, 0.2, 1] };
/** 页签换页退场 */
const TAB_OUT: Transition = { duration: 0.12, ease: "easeOut" };

/**
 * 页签切换的内容层。`custom` 是方向（±1：点的那一档在页签行里偏左还是偏右），
 * 新内容从点击侧进、旧内容往反方向出，与上方选中胶囊的横向滑动同一条轴。
 *
 * 必须配 `<AnimatePresence mode="popLayout" initial={false} custom={dir}>`：
 *  - `popLayout` 把退场层抽离文档流，容器高度当场就是新内容的（用 `mode="wait"` 会先塌一次
 *    再弹回来——概况与方案两签差着好几屏）；
 *  - `exit` 读的是 **AnimatePresence 上的 custom**：退场层是上一次渲染留下的快照，它自己那份
 *    参数已经过期，只有 AnimatePresence 是当前这一次渲染的。
 *
 * 横向只走 18px，而 `main` 左右各 32px padding，退场层也仍在其内 ⇒ 撑不出横向滚动条。
 */
export const TAB_SWEEP: Variants = {
    hidden: (dir: number) => ({ opacity: 0, x: dir * 18, transition: TAB_IN }),
    show: { opacity: 1, x: 0, transition: TAB_IN },
    exit: (dir: number) => ({ opacity: 0, x: dir * -18, transition: TAB_OUT }),
};

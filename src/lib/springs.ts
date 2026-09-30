/**
 * 全站 motion 弹簧唯一出处（11 条弹簧、16 个用点，数值是各处原值）。
 * 硬约束：这里的弹簧都偏硬（越线不到 1%），大位移（列表重排 / 换页 / 卡片）一律不走弹簧——
 * 要"弹"改用 `src/lib/entry-curve.ts` 里峰值固定 px 的曲线。
 * 手势甩出的续动只能写 stiffness/damping/mass——写成 visualDuration 或 duration 会把继承速度清零。
 * 不要只写 visualDuration 不写 bounce：motion 会静默忽略前者，回落成默认弹簧，手感变味。
 */
import type { Transition } from "motion/react";

/** 开关滑块走 14px：硬，到位时刚磕一下壁 */
export const TOGGLE_SLIDE: Transition = {
    type: "spring",
    stiffness: 500,
    damping: 28,
    mass: 0.9,
};
/** 开关松手收回横向拉伸：比行程再硬一档，与位移互不干扰 */
export const TOGGLE_PRESS: Transition = { type: "spring", stiffness: 700, damping: 30 };

/** 选中胶囊滑动：分段页签的三档、侧栏三个分类的导航行共用（同一件事——一块淡底在候选之间滑） */
export const PILL_SLIDE: Transition = { type: "spring", stiffness: 420, damping: 36 };
/** 计数滚动（旧数上滑、新数升入）：16px 微行程 */
export const COUNT_ROLL: Transition = { type: "spring", stiffness: 420, damping: 32 };

/** 卡片与列表行入场上浮 14~24px：转换页卡、帮助三卡、目录行插入共用 */
export const RISE: Transition = { type: "spring", stiffness: 320, damping: 28 };
/** 首页右列推入与 Dropzone 大小卡形变：位移大，所以更软 */
export const MORPH: Transition = { type: "spring", stiffness: 260, damping: 28 };
/** 拖放区被托起 */
export const LIFT: Transition = { type: "spring", stiffness: 380, damping: 28 };
/** 首页实况小窗从下方推入 72px */
export const RAIL_RISE: Transition = { type: "spring", stiffness: 240, damping: 28 };

/** 方案行的补位：不排队，后面的行立刻跟上 */
export const PLAN_LAYOUT: Transition = {
    type: "spring",
    stiffness: 420,
    damping: 34,
    mass: 0.7,
};
/** 方案行落位：轻微回弹，到位时「顿」一下 */
export const PLAN_LAND: Transition = {
    type: "spring",
    stiffness: 400,
    damping: 28,
    mass: 0.8,
};
/** 任务卡删除后的补位：位移大的自己跑得快，到位时间近似恒定 */
export const REFLOW: Transition = { type: "spring", stiffness: 340, damping: 26, mass: 1 };

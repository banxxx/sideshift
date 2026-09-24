/**
 * 全站 motion 弹簧的唯一出处：14 个用点、11 条弹簧，数值是各处原本就在用的原值。
 *
 * 改之前先读这三条，它们都是踩出来的：
 *  1. 弹簧的过冲距离与位移成正比（行程 400px 就会飞过头约 100px）。所以列表重排、换页、
 *     卡片大位移一律不走这里的弹簧，改用 src/features/tasks/entry-curve.ts 里自算的「送+弹」曲线。
 *  2. 手势甩出去的续动只能用 stiffness/damping/mass。写成 visualDuration 或 duration 的
 *     话，motion 会把继承速度强行清零，甩动直接没有反应。
 *  3. 不要只写 visualDuration 而不写 bounce —— motion 会静默忽略前者，
 *     回落成 stiffness 100 / damping 10 的默认弹簧，手感完全变味。
 *
 * 需要错峰的地方自己补 delay（见 PLAN_LAND、REFLOW 的用法）。
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

/** 分段页签选中胶囊滑动：短促、不回弹 */
export const SEG_PILL: Transition = { type: "spring", stiffness: 420, damping: 36 };
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

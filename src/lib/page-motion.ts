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
 * 局部换一批（SWAP，见 @/components/ui/Swap）再缩一档：6px、进 180 / 出 90，指尖还停在原地。
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

/**
 * 局部换一批入场：比页签再小一档——指尖没离开这块区域，不该读出「换了一屏」。
 * 也导出来给「收放一格」那类用（`Collapse axis="x"`，如标题栏返回件）：那里一条 transition 管三条属性
 * （见 Collapse 文件头），进=出、不分轨，所以取这一档而不是 COLLAPSE 的 300ms。
 */
export const SWAP_IN: Transition = { duration: 0.18, ease: [0.2, 0.8, 0.2, 1] };
/** 局部换一批退场：让位给新内容，旧的那一批只是消失，不等它 */
const SWAP_OUT: Transition = { duration: 0.09, ease: "easeOut" };

/**
 * 同一位置换一批 DOM（状态按钮组、清单↔空态、标题栏返回件）的进出场。
 * 纵向 6px：这里的语义是「新的一批落位」，不是页签那种「往旁边翻一页」，所以不横向走。
 *
 * 搭配见 @/components/ui/Swap（popLayout + initial={false} + 自带 relative 外壳，原因写在那儿）。
 */
export const SWAP: Variants = {
    hidden: { opacity: 0, y: 6, transition: SWAP_IN },
    show: { opacity: 1, y: 0, transition: SWAP_IN },
    exit: { opacity: 0, y: -4, transition: SWAP_OUT },
};

/**
 * 卡内「整块条件挂卸」的进出场（见 @/components/ui/Collapse）。
 *
 * 300ms 进=出、同一条对称曲线、淡入淡出走满全程、内容不位移——这四个值是他在
 * `.scratch/collapse-proto.html` 里拧出来的（快照原文：`durIn=300 durOut=300
 * ease=cubic-bezier(.2,.8,.2,1) fade=100% phase=sync y=0`），要改手感只改这一行。
 *
 * 为什么和上面三档节拍都不同：换页与换批是「指尖已经点了下一处」的语义，要短；
 * 折叠是**同一条内容自己长出来**，读的是「这块地方有了/没了」，短到 180ms 就退化成硬跳。
 * 三条属性（height / marginTop / opacity）共用这一条 transition，所以淡入淡出天然走满全程。
 */
export const COLLAPSE: Transition = { duration: 0.3, ease: [0.2, 0.8, 0.2, 1] };

/* ===================== 波浪前缘的刹车曲线（主题切换那条圆形波的 JS 副本） =====================
 * 逐字抄 `src/App.css` 的 `@keyframes theme-reveal` 的刹车段：起始斜率 2.0 ⇒ 一撤就动，
 * 末端斜率 0.013 ⇒ 最后那点是爬回去的。App.css 那一头改了这条必须跟着改——两处没有编译期
 * 联系，只有这段注释。（原曲线前 22% 时间还有一段**等速**走完 36% 半径，那是给横跨整片的
 * 大行程保匀速观感用的，单卡十几度用不上。）
 *
 * 这里原来还住着「半径 → 第几毫秒被扫到」那条反函数（`waveFrontTime` / `waveFrontDurMs`）
 * 和它的两个分段常数：只有鸣谢名单的涟漪读它。涟漪撤成头像自转之后没有读者，一并删净——
 * 要复原波浪，得连那两个函数一起从 git 里捞回来，只留这一条曲线是不够的。
 */
export const WAVE_FRONT_EASE = "cubic-bezier(0.16, 0.32, 0.25, 0.99)";



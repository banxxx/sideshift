/**
 * 卡片换页时自己的「升起 + 回弹」位移曲线，多页共用（模式 C 换页语义：在场每张卡一律从下方 `risePx` 升起、演同一条曲线，谁都不回原格子；被筛掉的当场从 DOM 消失，不做下落退场层）。
 * 不走 motion 的 layout 弹簧（过冲按行程等比，参数调不平两种切换）：「送」与「弹」拆开，弹段是独立的衰减正弦，峰值先归一化再乘振幅 ⇒ 与行程无关。
 * 时长口径的数字由 `.scratch/__entrycurve.mjs` 从下面的真实常量算出，改参数就重跑它。
 */
import { prefersReducedMotion } from "./utils";

export const ENTRY = {
    /** 从下方多远处升起（也就是这一趟的行程：换页语义下人人相同） */
    risePx: 30,
    /** 越过目标线多远：这就是「回弹」本身 */
    ampPx: 16,
    /** 回弹更轻的一档：整页只有卡片在动、没有补位要压住时用 */
    softAmpPx: 8,
    /** 「弹」那一段的时长：整条时间轴跟着它等比伸缩，是「动效有多长」的唯一旋钮 */
    tailMs: 620,
    /** 「送」那一段按行程补的时间：每 100px 多花这么多 ms */
    extraMsPer100px: 30,
    /** 半波个数：1 = 冲过线就定住；1.6 = 带回摆（线下 2.45px，看得清又不读成往下走） */
    cycles: 1.6,
    /** 尾振衰减，越大收得越快；调小回摆变深 */
    decay: 3.0,
    fadeMs: 200,
    /** 错峰总长：卡片再多也不排队，步长按在场张数回算 */
    waveMs: 442,
    stepMinMs: 6,
    stepMaxMs: 34,
    /** reduced-motion：只留一记淡入，位移与错峰全剥掉 */
    reducedFadeMs: 140,
} as const;

/** 密集采样：位移 = 行程 × 送段 + 振幅 × 弹段（弹段峰值归一化，故与行程无关；弹只朝「越过目标线」那一侧） */
function unified(travel: number, ampPx: number) {
    const { tailMs, extraMsPer100px, cycles, decay } = ENTRY;
    const moveMs = Math.abs(travel) * (extraMsPer100px / 100);
    const total = Math.max(60, moveMs + tailMs);
    const split = moveMs / total;
    const n = Math.max(48, Math.round(total / (1000 / 90)));
    const wave = new Array<number>(n + 1);
    let peak = 0;
    for (let i = 0; i <= n; i++) {
        const q = i / n <= split ? 0 : (i / n - split) / (1 - split);
        wave[i] = q === 0 ? 0 : -Math.exp(-decay * q) * Math.sin(cycles * Math.PI * q);
        if (Math.abs(wave[i]) > peak) peak = Math.abs(wave[i]);
    }
    const pts: { v: number }[] = [];
    for (let i = 0; i <= n; i++) {
        const p = i / n;
        const ride = split > 0 && p < split ? Math.pow(1 - p / split, 1.8) : 0;
        pts.push({ v: travel * ride + (peak ? ampPx * (wave[i] / peak) : 0) });
    }
    pts[pts.length - 1].v = 0;
    return { pts, total };
}

/** 降采样成 WAAPI 关键帧：缓动已烘进数值里，逐段只能 linear */
function bake(pts: { v: number }[]): Keyframe[] {
    const step = Math.max(1, Math.round(pts.length / 70));
    const out: Keyframe[] = [];
    for (let i = 0; i < pts.length; i += step) {
        out.push({ offset: i / (pts.length - 1), transform: `translateY(${pts[i].v.toFixed(3)}px)`, easing: "linear" });
    }
    out.push({ offset: 1, transform: `translateY(${pts[pts.length - 1].v.toFixed(3)}px)` });
    return out;
}

/** 行程恒定、按回弹幅度各烘一条：同一个幅度只烘一次，之后每次换页复用 */
const CURVES = new Map<number, { frames: Keyframe[]; total: number }>();
function curveOf(ampPx: number) {
    let c = CURVES.get(ampPx);
    if (!c) {
        const curve = unified(ENTRY.risePx, ampPx);
        c = { frames: bake(curve.pts), total: curve.total };
        CURVES.set(ampPx, c);
    }
    return c;
}

/** 错峰步长：把 waveMs 摊到在场张数上，再多也不拖成长队 */
export function entryStepMs(count: number): number {
    if (count < 2) return 0;
    return Math.min(ENTRY.stepMaxMs, Math.max(ENTRY.stepMinMs, ENTRY.waveMs / (count - 1)));
}

/**
 * 让一张卡片演一次「升起 + 弹同一下」（`ampPx` 回弹幅度，省略即 `ENTRY.ampPx`）。`fill: "backwards"` 是必需的：没有它，排在错峰队列后面的卡片
 * 会在自己那一段开始前沿着最终位置亮着，到点才跳回起点。
 * 返回撤销函数：连续切页签时把上一批掐掉，免得两条曲线在同一节点上打架。
 * 掐掉就是「落在终点」——曲线终点正是 translateY(0)/opacity 1，等于卡片本来所在的位置，不会跳。
 */
export function playEntry(el: HTMLElement, delayMs: number, ampPx: number = ENTRY.ampPx): () => void {
    const reduced = prefersReducedMotion();
    const { frames, total } = curveOf(ampPx);
    const anims = reduced
        ? [el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ENTRY.reducedFadeMs, easing: "ease-out" })]
        : [
              el.animate(frames, {
                  duration: total,
                  delay: delayMs,
                  easing: "linear",
                  fill: "backwards",
              }),
              el.animate([{ opacity: 0 }, { opacity: 1 }], {
                  duration: ENTRY.fadeMs,
                  delay: delayMs,
                  easing: "ease-out",
                  fill: "backwards",
              }),
          ];
    return () => anims.forEach((a) => a.cancel());
}

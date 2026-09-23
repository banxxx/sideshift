/**
 * 换筛选时卡片自己的位移曲线（数字 = `.scratch/tab-spring-proto.html` 的「果冻（定案 · 波数 1）」预设，模式 C）。
 *
 * **换页语义**：切页签演的是「换了一屏内容」，谁都不回自己原来的格子 —— 在场每张卡片（不管是留下的
 * 还是新挂载的）一律从下方 `risePx` 升起、演同一条曲线。曲线本身单调向上（波数 1，弹过目标线就定住，
 * 不再摆回线下），所以卡片不会有一段反着走的位移；错峰会让排在后面的卡片先在下方停一会儿等自己那一段，
 * 那是等待不是往回走。被筛掉的卡片当场从 DOM 消失：不做下落退场层（试过：那张克隆会盖在页面
 * 上，用户判「难看」，别再往回改）。
 *
 * 为什么不走 motion 的 layout 弹簧：弹簧的过冲 = 阻尼比定下的比例 × **行程**，按行程等比，参数调不平
 * 两种切换。这里把「送」和「弹」拆开：弹那一段是独立的衰减正弦，**峰值先归一化成 1 再乘振幅**
 * ⇒ 与卡片挪多远无关，固定 px、固定 ms。
 */
import { prefersReducedMotion } from "./delete-flight";

export const ENTRY = {
    /** 从下方多远处升起（也就是这一趟的行程：换页语义下人人相同） */
    risePx: 26,
    /** 越过目标线多远：这就是「回弹」本身 */
    ampPx: 16,
    /** 「弹」那一段的时长 */
    tailMs: 620,
    /** 「送」那一段按行程补的时间：每 100px 多花这么多 ms */
    extraMsPer100px: 30,
    /** 半波个数：1 = 只弹一次定住，>1 = 弹过之后再往回摆到线下（2.2 时线下 6.45px，用户判「往下走」） */
    cycles: 1.0,
    /** 尾振衰减，越大收得越快 */
    decay: 2.0,
    fadeMs: 150,
    /** 错峰总长：卡片再多也不排队，步长按在场张数回算 */
    waveMs: 340,
    stepMinMs: 6,
    stepMaxMs: 26,
    /** reduced-motion：只留一记淡入，位移与错峰全剥掉 */
    reducedFadeMs: 140,
} as const;

/** 密集采样：位移 = 行程 × 送段 + 振幅 × 弹段（弹段峰值归一化，故与行程无关） */
function unified(travel: number) {
    const { ampPx, tailMs, extraMsPer100px, cycles, decay } = ENTRY;
    const moveMs = Math.abs(travel) * (extraMsPer100px / 100);
    const total = Math.max(60, moveMs + tailMs);
    const split = moveMs / total;
    const sign = travel < -0.5 ? -1 : 1;
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
        pts.push({ v: travel * ride + (peak ? sign * ampPx * (wave[i] / peak) : 0) });
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

/** 全场只有这一条曲线：行程恒定，模块加载时烘一次就够 */
const CURVE = unified(ENTRY.risePx);
const MOVE_KEYFRAMES = bake(CURVE.pts);

/** 错峰步长：把 waveMs 摊到在场张数上，再多也不拖成长队 */
export function entryStepMs(count: number): number {
    if (count < 2) return 0;
    return Math.min(ENTRY.stepMaxMs, Math.max(ENTRY.stepMinMs, ENTRY.waveMs / (count - 1)));
}

/** 演完整趟：曲线本身 + 错峰满长 + 一点余量。调用方拿它排「什么时候把 layout 投影打开」 */
export const ENTRY_RUN_MS = Math.round(CURVE.total + ENTRY.waveMs + 120);

/**
 * 让一张卡片演一次「升起 + 弹同一下」。`fill: "backwards"` 是必需的：没有它，排在错峰队列后面的卡片
 * 会在自己那一段开始前沿着最终位置亮着，到点才跳回起点。
 * 返回撤销函数：连续切页签时把上一批掐掉，免得两条曲线在同一节点上打架。
 */
export function playEntry(el: HTMLElement, delayMs: number): () => void {
    const reduced = prefersReducedMotion();
    const anims = reduced
        ? [el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ENTRY.reducedFadeMs, easing: "ease-out" })]
        : [
              el.animate(MOVE_KEYFRAMES, {
                  duration: CURVE.total,
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

/**
 * 删除动画：卡片沿弧线飞进右下角的垃圾桶。
 *
 * 弧线用「二次贝塞尔升级成的三次」，推进按**弧长**而不是参数 t（否则直段跑快、弯段跑慢）；
 * 节奏是 `flightProg` 那两拍（甩出滑行 → 桶口停顿吸入），不是一条匀速曲线。
 * 缓动全部在 JS 里烘焙进关键帧、WAAPI 一律 linear —— WAAPI 的 options.easing 是逐段生效的，
 * 几十个关键帧再配全局缓动会抖。
 *
 * 这里只管「飞」这一段 DOM：克隆卡片、动画、追回。列表补位（FLIP 的等价物）由 TasksPage 用
 * motion 的 layout 弹簧做；数据什么时候真的删掉，由调用方在 lead 回调里决定。
 */

export interface Pt {
    x: number;
    y: number;
}

/** 垃圾桶按钮的测量锚：飞行落点读它，永远挂载（空回收站时只是透明） */
export const TRASH_BIN_ATTR = "data-trash-bin";

export const FLIGHT = {
    /** 整段飞行时长：慢到眼睛跟得住全程，又不像卡住 */
    durationMs: 640,
    /** 弧顶外凸（px），沿起终点连线的法线方向 */
    bowPx: 110,
    /** 提前量：落地前这么多毫秒就把位置腾出来，「吸进桶」和下方补位重叠发生。
     *  这条也管收尾卡顿：lead 会触发一次 React 摘除 + 下方全部卡片重排，压得太晚就把掉帧
     *  堆在飞行最后几十毫秒上（看着就是「收尾卡一下才消失」）。 */
    leadMs: 170,
    /** 旋转终值（deg），按进度平方推进——前段几乎不转，末段才甩 */
    rotDeg: -24,
    scaleTo: 0.24,
} as const;

/** reduced-motion：飞行收成一记淡出，位移/缩放/旋转全剥掉（列表补位由 MotionConfig 代管） */
export function prefersReducedMotion(): boolean {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function center(r: DOMRect): Pt {
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
}

/** 垃圾桶中心；拿不到（未挂载）时返回 null，调用方按「就地淡出」处理 */
export function binCenter(): Pt | null {
    const el = document.querySelector<HTMLElement>(`[${TRASH_BIN_ATTR}]`);
    return el ? center(el.getBoundingClientRect()) : null;
}

/* ---------------- 曲线数学 ---------------- */

interface Curve {
    c1: Pt;
    c2: Pt;
    end: Pt;
    /** 弧长表缓存 */
    tab?: { cum: number[]; total: number; n: number };
}

/** from→to 的弧线：先取垂直于连线、外凸 bowPx 的二次控制点，再等价升级成三次贝塞尔 */
function curveTo(to: Pt, bow: number): Curve {
    const dx = to.x,
        dy = to.y;
    const len = Math.hypot(dx, dy) || 1;
    // 法线方向（单位向量），让弧顶恒偏向连线的一侧
    const nx = -dy / len,
        ny = dx / len;
    const cx = dx / 2 + nx * bow,
        cy = dy / 2 + ny * bow;
    // 二次 → 三次：C1 = S + 2/3(C−S)，C2 = E + 2/3(C−E)
    return {
        c1: { x: cx * (2 / 3), y: cy * (2 / 3) },
        c2: { x: dx + (cx - dx) * (2 / 3), y: dy + (cy - dy) * (2 / 3) },
        end: { x: dx, y: dy },
    };
}

function bez(p: Curve, t: number): Pt {
    const u = 1 - t;
    return {
        x: 3 * u * u * t * p.c1.x + 3 * u * t * t * p.c2.x + t * t * t * p.end.x,
        y: 3 * u * u * t * p.c1.y + 3 * u * t * t * p.c2.y + t * t * t * p.end.y,
    };
}

function arcTable(p: Curve) {
    if (p.tab) return p.tab;
    const n = 240;
    const cum = [0];
    let prev = { x: 0, y: 0 };
    for (let i = 1; i <= n; i++) {
        const q = bez(p, i / n);
        cum.push(cum[i - 1] + Math.hypot(q.x - prev.x, q.y - prev.y));
        prev = q;
    }
    p.tab = { cum, total: cum[n], n };
    return p.tab;
}

/** 按弧长分数取点：offset-path 天生等距推进，手写关键帧必须自己补这一层 */
function posAt(p: Curve, frac: number): Pt {
    const { cum, total, n } = arcTable(p);
    const target = total * Math.max(0, Math.min(1, frac));
    let lo = 0,
        hi = n;
    while (lo < hi) {
        const m = (lo + hi) >> 1;
        if (cum[m] < target) lo = m + 1;
        else hi = m;
    }
    const i = Math.max(1, lo);
    const span = cum[i] - cum[i - 1] || 1;
    const u = (target - cum[i - 1]) / span;
    const a = bez(p, (i - 1) / n);
    const b = bez(p, i / n);
    return { x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u };
}

/** cubic-bezier 求值：把缓动烘焙进关键帧用 */
function makeEase(str: string): (x: number) => number {
    const m = /cubic-bezier\(([^)]+)\)/.exec(str);
    if (!m) return (x) => x;
    const [x1, y1, x2, y2] = m[1].split(",").map(Number);
    const bx = (t: number) => 3 * (1 - t) * (1 - t) * t * x1 + 3 * (1 - t) * t * t * x2 + t ** 3;
    const by = (t: number) => 3 * (1 - t) * (1 - t) * t * y1 + 3 * (1 - t) * t * t * y2 + t ** 3;
    return (x) => {
        let lo = 0,
            hi = 1,
            t = x;
        for (let i = 0; i < 24; i++) {
            t = (lo + hi) / 2;
            if (bx(t) < x) lo = t;
            else hi = t;
        }
        return by(t);
    };
}

/** 追回：先快后缓地弹回原位 */
const EASE_RECALL = makeEase("cubic-bezier(.2,.72,.28,1)");

/** 落点保底速度（弧长占比 / 时间占比）：末速**不给 0**。
 *  停在桶口再跳走 = 「收尾卡一下、然后突然没了」，这条就是那记卡顿的来源。 */
const ARRIVE_SLOPE = 0.18;

/**
 * 飞行节奏：时间 → 弧长。整条都是「越近越慢」的减速，但不许停死。
 * 做法 = 二次 ease-out（起手最快、末速 0）掺进一点匀速：
 *   前 10% 时间走 ~17% 弧长（眼睛跟得住，不是旧版那种一帧跑掉 1/3），
 *   末 10% 时间还剩 2% 弧长在动，所以淡出与落位是同时到的，看不见「消失」那一刀。
 */
function flightProg(u: number): number {
    const glide = 1 - (1 - u) ** 2;
    return (1 - ARRIVE_SLOPE) * glide + ARRIVE_SLOPE * u;
}

/** 缩放关键帧（[进度, 值]）：全程收，末了收到桶口大小 */
const SCALE_KEYS: Array<[number, number]> = [
    [0, 1],
    [0.45, 0.82],
    [0.75, 0.5],
    [1, FLIGHT.scaleTo],
];
/** 透明度跟的是**弧长**而不是时间：越接近桶越虚，正好在落位那一帧到 0——
 *  「消失」被摊进整段接近过程里，就不会出现看着看着卡片凭空掉了一刀。 */
const OPACITY_KEYS: Array<[number, number]> = [
    [0, 1],
    [0.35, 0.9],
    [0.6, 0.7],
    [0.8, 0.42],
    [0.92, 0.18],
    [1, 0],
];

function at(keys: Array<[number, number]>, t: number): number {
    for (let i = 1; i < keys.length; i++) {
        if (t <= keys[i][0]) {
            const [a, va] = keys[i - 1];
            const [b, vb] = keys[i];
            return va + (vb - va) * (b === a ? 1 : (t - a) / (b - a));
        }
    }
    return keys[keys.length - 1][1];
}

/* ---------------- 飞行 ---------------- */

export type FlightOutcome = "drop" | "recall";

export interface Flight {
    /** 落地（drop）或被追回（recall）后 resolve */
    readonly done: Promise<FlightOutcome>;
    /** 列表重绘 / 组件卸载：作废这趟飞行，收尾不再落到新状态上 */
    cancel(): void;
}

let flightHost: HTMLDivElement | null = null;

function host(): HTMLDivElement {
    if (flightHost) return flightHost;
    flightHost = document.createElement("div");
    flightHost.dataset.flightLayer = "";
    // 整层不吃事件，只有飞行卡片本身吃：点飞行中的卡片 = 追回（lead 之后关掉，见 gateAndLead）
    flightHost.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:80";
    document.body.appendChild(flightHost);
    return flightHost;
}

const live = new Set<Flight>();

/** 作废全部在飞（列表整体重建时用） */
export function abortAllFlights(): void {
    [...live].forEach((f) => f.cancel());
}

/**
 * 让卡片飞进垃圾桶。原卡片由调用方负责隐藏（保持占位）与最终摘除。
 */
export function startFlight(el: HTMLElement, onLead?: () => void): Flight {
    const reduced = prefersReducedMotion();
    const rect = el.getBoundingClientRect();
    const start = center(rect);
    const to = binCenter() ?? start;
    const dur = reduced ? 140 : FLIGHT.durationMs;
    const bow = reduced ? 0 : FLIGHT.bowPx;
    /** 提前量对两条路同样成立：reduced 的 `dur` 只有 140ms，`dur * .6` 这个夹子会自动把它
     *  收成 84ms，于是「腾位置」照样落在淡出的尾巴上（56ms 处），与真实飞行同一条规则。
     *  以前这里写死 `reduced ? 0`，配合下面定时器的短路就等于「reduced 下 lead 永不发生」。 */
    const leadMs = Math.max(0, Math.min(FLIGHT.leadMs, dur * 0.6));

    const cl = el.cloneNode(true) as HTMLElement;
    cl.removeAttribute("id");
    // 卡片可能正被 motion 的 layout 弹簧推着补位，克隆会带进那一帧 transform——位置改由 left/top 表达
    cl.style.cssText += `;position:fixed;left:${rect.left}px;top:${rect.top}px;width:${
        rect.width
    }px;height:${rect.height}px;margin:0;transform:none;transform-origin:50% 50%;pointer-events:auto;cursor:pointer;box-shadow:0 18px 40px rgba(0,0,0,.28);will-change:transform,opacity`;
    cl.setAttribute("aria-hidden", "true");
    cl.querySelectorAll("button").forEach((b) => b.setAttribute("tabindex", "-1"));
    host().appendChild(cl);

    const curve = curveTo({ x: to.x - start.x, y: to.y - start.y }, bow);
    const frameAt = (time: number, prog: number): Keyframe =>
        reduced
            ? { offset: time, opacity: 1 - time }
            : {
                  offset: time,
                  opacity: at(OPACITY_KEYS, prog),
                  transform: (() => {
                      const q = posAt(curve, prog);
                      return `translate(${q.x.toFixed(1)}px, ${q.y.toFixed(
                          1
                      )}px) scale(${at(SCALE_KEYS, prog).toFixed(3)}) rotate(${(
                          FLIGHT.rotDeg *
                          prog *
                          prog
                      ).toFixed(2)}deg)`;
                  })(),
              };

    /** 关键帧的自变量永远是「时间占比」，节奏全在 progOf 里 */
    const bake = (progOf: (time: number) => number, n: number): Keyframe[] => {
        const kf: Keyframe[] = [];
        for (let i = 0; i <= n; i++) {
            const time = i / n;
            kf.push(frameAt(time, progOf(time)));
        }
        return kf;
    };

    let anim = cl.animate(bake(flightProg, 40), {
        duration: dur,
        easing: "linear",
        fill: "forwards",
    });
    // currentTime 的类型是 CSSNumberish（可能是字符串），数值时间轴下取到的一定是 ms
    const progNow = () => flightProg(Math.min(1, (Number(anim.currentTime) || 0) / dur));

    let resolve!: (o: FlightOutcome) => void;
    const done = new Promise<FlightOutcome>((r) => (resolve = r));
    let finished = false;
    let recalled = false;

    /** lead 之后克隆必须彻底不吃事件：删除已经提交，这时「追回」不再是「取消这一下」，
     *  而是「从回收站撤回一条已提交的删除」——那件事该由看得见的按钮做（弹窗里的「撤回」、或 Esc）。
     *  留着它就是个隐形陷阱：落点正是桶心，`scale()` 连命中区一起缩，末段那张卡片盖住垃圾桶按钮
     *  中央那条横带（z-80 高于按钮的 z-40），而 opacity 已经走到 0.18→0。点下去既不开弹窗、
     *  又把刚删的任务要了回来。键盘那条路（下面的 Escape）不依赖 pointer-events，照旧可用。
     *  reduced 路没有位移、压不到按钮，闸门照样落下：封口条件统一成「一提交就不可点」，不分两条路。 */
    const gateAndLead = () => {
        cl.style.pointerEvents = "none";
        cl.style.cursor = "default";
        onLead?.();
    };
    /* 调用方唯一提交删除的入口，两条动画路都必须走到：这里原先挂着 `reduced ||`，
       于是「减弱动态效果」下 onLead 永不触发 → commitDelete/setGone 全没跑，
       点删除只是原地淡出一下，卡片照旧留在列表，而垃圾桶已经亮起了乐观占位。 */
    const leadTimer = !onLead ? 0 : window.setTimeout(gateAndLead, dur - leadMs);

    const onKey = (e: KeyboardEvent) => {
        if (e.key === "Escape") recall();
    };
    window.addEventListener("keydown", onKey);
    cl.addEventListener("pointerdown", recall);

    /** 追回：不用 anim.reverse()（被打断处起步不稳），从当前弧长分数另起一段折回 0 */
    function recall() {
        if (recalled || finished) return;
        const p0 = progNow();
        if (p0 <= 0.02) return;
        recalled = true;
        window.clearTimeout(leadTimer);
        anim.cancel();
        anim = cl.animate(bake((u) => p0 * (1 - EASE_RECALL(u)), 20), {
            duration: reduced ? 120 : Math.max(200, dur * 0.45),
            easing: "linear",
            fill: "forwards",
        });
        anim.onfinish = () => finish("recall");
    }

    anim.onfinish = () => {
        if (!recalled) finish("drop");
    };

    function finish(outcome: FlightOutcome) {
        if (finished) return;
        finished = true;
        live.delete(flight);
        window.clearTimeout(leadTimer);
        window.removeEventListener("keydown", onKey);
        cl.remove();
        resolve(outcome);
    }

    const flight: Flight = {
        done,
        cancel() {
            if (finished) return;
            finished = true;
            live.delete(flight);
            window.clearTimeout(leadTimer);
            window.removeEventListener("keydown", onKey);
            try {
                anim.cancel();
            } catch {
                // 动画已被丢弃也得把克隆收掉，否则屏幕上留一张假卡片
            }
            cl.remove();
            resolve("recall");
        },
    };
    live.add(flight);
    return flight;
}

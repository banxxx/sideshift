/**
 * 撕票件的手写运动件（孔列几何 + 断裂判据 + 积分器），组件只管 DOM 与事件，分工与 drag-sort.ts 同一口径。
 * 硬约束一：孔列几何按**量到的宽高**重算——`clip-path: path()` 写的是 px，缩放适配会让撕口和卡宽脱钩。
 * 硬约束二：断裂吃的是**角度增量**（`want = wrap(a - a0) · sign · follow`），所以「沿铰点→抓取点那条射线径直往外拽」Δa≡0，
 * 票根一丝不撕；必须让指针绕铰点转出角来（抓票根上沿⇒铰在孔列下端⇒往右下拽才撕得动，而且票根是**向下**坠出去）。
 */

/** 他给的那份参数快照（`.scratch/tear-ticket-proto.html` 的终值）：几何档 */
export const TEAR = {
    /** 票根宽：撕口到右缘的距离 */
    stubSize: 80,
    /** 四角圆角，与卡的 `rounded-[12px]` 同档 */
    radius: 12,
    /** 孔数与孔径；孔列两端各留 `notch` 的空档，铰点就钉在这两个端点上 */
    holes: 7,
    holeSize: 5,
    notch: 3,
    /** 桥段的横向抖动（0 = 撕口是纯直线段 + 圆孔） */
    roughness: 0,
} as const;

/** 力学档 */
export const TEAR_PHYSICS = {
    /** 一条桥要撑开这么多像素才断（弦高判据的门限） */
    stretch: 18,
    /** 阻力：没断的桥越多越不跟手（满 intact 时只跟 `followMax·(1-resistance)` 倍） */
    resistance: 0.45,
    tearAngle: 30,
    followMax: 0.92,
    /** 全断后票根「挂着」的那一点角度，以及甩动给它的偏移 */
    hangRatio: 0.55,
    hangVel: 0.0009,
    hangVelClamp: 0.3,
    /** 坠落与回弹 */
    gravity: 2400,
    dropSpin: 1.2,
    velClamp: 1600,
    velClampDown: 1200,
    /** 松手前静止这么久就不带速度（慢慢放下 ≠ 甩出去） */
    stillMs: 80,
    /** 撕断的每一格拽主体回缩一下：冲量按总格数摊薄 */
    bodyK: 520,
    bodyC: 30,
    bodySnapImpulse: 560,
    bodyCutImpulse: 150,
    /** 回位弹簧（没撕完就松手） */
    returnK: 300,
    returnC: 24,
    /** 各段一阶低通的时间常数（秒）：`x += (target - x) · (1 - exp(-dt/tau))` */
    tauAngle: 0.035,
    tauSlide: 0.05,
    tauFreeSlide: 0.045,
    tauHang: 0.12,
    tauReturn: 0.07,
    /** 淡出：坠落满这么久才开始收，收这么久 */
    fadeAfter: 0.16,
    fadeDur: 0.42,
    /** 丝（fibres）：断口之间的连接纤维，撑到多长、垂多少、多粗 */
    retract: 0.17,
    fibreGapPx: 0.35,
    fibreSag: 0.18,
    fibreW: 1.7,
    fibreWK: 1.15,
    fibreOffset: 1.6,
    fibreRetractW: 0.9,
    /** 回位收敛判据 */
    settleTheta: 0.0008,
    settleThetaV: 0.01,
    settlePos: 0.05,
    /** 拽离方向的位移增益与夹持（`away` 只往右给一点，`side` 双向） */
    awayGain: 0.05,
    awayLo: -2,
    awayHi: 4,
    sideGain: 0.05,
    sideLo: -3,
    sideHi: 3,
} as const;

/** 收高那一拍的曲线：`COLLAPSE.ease = [0.2,0.8,0.2,1]` 的数值孪生（牛顿迭代解 x(u)=t） */
export function collapseEase(t: number): number {
    const cx = 0.6, bx = 0, ax = 0.4;
    const cy = 2.4, by = -1.8, ay = 0.4;
    const u0 = t <= 0 ? 0 : t >= 1 ? 1 : t;
    let u = u0;
    for (let i = 0; i < 8; i += 1) {
        const x = ((ax * u + bx) * u + cx) * u - u0;
        const d = (3 * ax * u + 2 * bx) * u + cx;
        if (Math.abs(d) < 1e-7) break;
        u = Math.min(1, Math.max(0, u - x / d));
    }
    return ((ay * u + by) * u + cy) * u;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const rad = (d: number) => (d * Math.PI) / 180;
/** 把角度折回 (-π, π]：`a - a0` 跨 ±180° 时不能直接比大小 */
const wrap = (a: number) => Math.atan2(Math.sin(a), Math.cos(a));
const f2 = (n: number) => n.toFixed(2);

/** 内置 PRNG：桥段的抖动按「孔数 + 卡高」定种，同一张卡每次重建拿到**同一**条撕口 */
const noise = (seed: number) => {
    let s = seed | 0;
    return () => {
        s = (s + 0x6d2b79f5) | 0;
        let t = Math.imul(s ^ (s >>> 15), 1 | s);
        t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
};

export interface TearPoint {
    x: number;
    y: number;
}

/** 一段桥（两个孔之间的实体连接）：`mid` 是断裂判据用的弦中点，`pts` 是画进 clip 路径的抖动折点 */
export interface TearBridge {
    x: number;
    y: number;
    y0: number;
    y1: number;
    mid: number;
    pts: [number, number][];
}

export interface TearGeometry {
    width: number;
    height: number;
    /** 左右两半各自的 `clip-path: path()` 字符串（同一套桥，反向各描一遍） */
    body: string;
    stub: string;
    bridges: TearBridge[];
    /** 孔列两端 = 两个候选铰点；`v` 是它在孔列坐标里的竖向位置 */
    ends: (TearPoint & { v: number })[];
}

/**
 * 一张卡的孔列几何。主体与票根共用同一条孔列：主体那半把孔咬成向内凹的弧，票根那半反向咬，
 * 两半沿同一条折线分家——这就是「撕开之后两边都留锯齿」的来历。
 */
export function buildTearGeometry(width: number, height: number): TearGeometry {
    const { stubSize, radius, holes, holeSize, notch, roughness } = TEAR;
    const main = width;
    const cross = height;
    const x = main - stubSize;
    const hr = holeSize / 2;
    const n = Math.max(1, Math.round(holes));
    const span = cross - 2 * notch;
    const bridge = Math.max(2, (span - n * holeSize) / (n + 1));
    const random = noise(n * 7919 + Math.round(cross));
    const pt = (u: number, v: number) => `${f2(u)},${f2(v)}`;
    const arc = (r: number, sweep: 0 | 1, u: number, v: number) =>
        `A${f2(r)},${f2(r)} 0 0 ${sweep} ${pt(u, v)}`;

    const bridges: TearBridge[] = [];
    for (let i = 0; i <= n; i += 1) {
        const y0 = notch + i * (bridge + holeSize);
        const y1 = y0 + bridge;
        const steps = Math.max(2, Math.round(bridge / 2.2));
        const pts: [number, number][] = [];
        for (let k = 1; k < steps; k += 1) {
            pts.push([x + (random() - 0.5) * 2 * roughness, y0 + (bridge * k) / steps]);
        }
        bridges.push({ x, y: (y0 + y1) / 2, y0, y1, mid: (y0 + y1) / 2, pts });
    }

    let body = `M${pt(radius, 0)}L${pt(x - notch, 0)}${arc(notch, 0, x, notch)}`;
    bridges.forEach((b, i) => {
        b.pts.forEach((p) => (body += `L${pt(p[0], p[1])}`));
        body += `L${pt(x, b.y1)}`;
        if (i < n) body += arc(hr, 0, x, b.y1 + holeSize);
    });
    body += `${arc(notch, 0, x - notch, cross)}L${pt(radius, cross)}${arc(radius, 1, 0, cross - radius)}L${pt(0, radius)}${arc(radius, 1, radius, 0)}Z`;

    let stub = `M${pt(x + notch, 0)}L${pt(main - radius, 0)}${arc(radius, 1, main, radius)}L${pt(main, cross - radius)}${arc(radius, 1, main - radius, cross)}L${pt(x + notch, cross)}${arc(notch, 0, x, cross - notch)}`;
    for (let i = n; i >= 0; i -= 1) {
        const b = bridges[i];
        for (let k = b.pts.length - 1; k >= 0; k -= 1) stub += `L${pt(b.pts[k][0], b.pts[k][1])}`;
        stub += `L${pt(x, b.y0)}`;
        if (i > 0) stub += arc(hr, 0, x, b.y0 - holeSize);
    }
    stub += `${arc(notch, 0, x + notch, 0)}Z`;

    return {
        width,
        height,
        body,
        stub,
        bridges,
        ends: [
            { x, y: notch, v: notch },
            { x, y: cross - notch, v: cross - notch },
        ],
    };
}

export type TearPhase = "idle" | "held" | "free" | "drop" | "return";

export interface TearSim {
    phase: TearPhase;
    pointerId: number | null;
    /** 铰点在抓取点的哪一侧：抓上沿 ⇒ 铰在下端 ⇒ 往右下拽为正角（票根向下坠） */
    sign: number;
    hinge: TearPoint;
    hingeV: number;
    grab: TearPoint;
    start: TearPoint;
    point: TearPoint;
    a0: number;
    /** 票根绕铰点转过的角（绝对值；倒向由 `sign` 给） */
    theta: number;
    thetaV: number;
    /** 票根的平移（拽离 + 侧移；`free` 之后是「手指已松开、票根还挂着」的那点余量） */
    sx: number;
    sy: number;
    vx: number;
    vy: number;
    spin: number;
    pvx: number;
    pvy: number;
    pointAt: number;
    fade: number;
    age: number;
    bx: number;
    bv: number;
    snapped: boolean[];
    snapAt: number[];
    span: number[];
    /** 这一帧刚撕完（淡尽）：组件据此藏票根并通知父层，随即清掉 */
    torn: boolean;
}

export function createTearSim(): TearSim {
    return {
        phase: "idle",
        pointerId: null,
        sign: 1,
        hinge: { x: 0, y: 0 },
        hingeV: 0,
        grab: { x: 0, y: 0 },
        start: { x: 0, y: 0 },
        point: { x: 0, y: 0 },
        a0: 0,
        theta: 0,
        thetaV: 0,
        sx: 0,
        sy: 0,
        vx: 0,
        vy: 0,
        spin: 0,
        pvx: 0,
        pvy: 0,
        pointAt: 0,
        fade: 1,
        age: 0,
        bx: 0,
        bv: 0,
        snapped: [],
        snapAt: [],
        span: [],
        torn: false,
    };
}

/** 抓在票根的上半还是下半，决定铰点落在孔列的哪一端（另一端当转轴） */
export function hingeFor(geo: TearGeometry, p: TearPoint): { sign: number; hinge: TearPoint; hingeV: number } {
    const far = p.y < geo.height / 2;
    const end = geo.ends[far ? 1 : 0];
    return { sign: far ? 1 : -1, hinge: { x: end.x, y: end.y }, hingeV: end.v };
}

/**
 * 按住票根。已经有角度（回位演到一半又按下去）就不重挑铰点，否则转轴会当场换一头。
 * `grab` 要把按下的那点**反旋回未撕状态**的坐标：`a0` 吃的是它，转过的角才不会一按下去就凭空多出一截。
 */
export function beginTear(s: TearSim, geo: TearGeometry, p: TearPoint, now: number): void {
    if (s.theta < 0.01) {
        // 从「整张完好」起步：上一趟演到一半松手的断裂位必须清掉，否则第二次撕会少断几格、当场就穿
        resetTear(s, geo);
        const h = hingeFor(geo, p);
        s.sign = h.sign;
        s.hinge = h.hinge;
        s.hingeV = h.hingeV;
    }
    const cos = Math.cos(-s.theta * s.sign);
    const sin = Math.sin(-s.theta * s.sign);
    const ux = p.x - s.sx - s.hinge.x;
    const uy = p.y - s.sy - s.hinge.y;
    s.grab = { x: s.hinge.x + ux * cos - uy * sin, y: s.hinge.y + ux * sin + uy * cos };
    // /0.92：`want` 那一路乘过 `followMax`，这里先除掉，按下那一瞬才不会自己挤进角度里
    s.a0 = Math.atan2(s.grab.y - s.hinge.y, s.grab.x - s.hinge.x) - (s.theta * s.sign) / TEAR_PHYSICS.followMax;
    s.start = p;
    s.point = p;
    s.pointAt = now;
    s.pvx = 0;
    s.pvy = 0;
    s.thetaV = 0;
    s.phase = "held";
}

/** 指针每动一次：喂位置与低通后的速度（速度只给坠落那一趟用，`held` 期间吃的是位置） */
export function feedTear(s: TearSim, p: TearPoint, now: number): void {
    const dt = Math.max(0.004, (now - s.pointAt) / 1000);
    s.pvx += ((p.x - s.point.x) / dt - s.pvx) * 0.35;
    s.pvy += ((p.y - s.point.y) / dt - s.pvy) * 0.35;
    s.pointAt = now;
    s.point = p;
}

/** 松手：`free`（全断了）就带速度坠出去，`held`（还挂着）就交回位弹簧弹回去 */
export function releaseTear(s: TearSim, now: number): void {
    if (s.phase === "free") {
        const still = now - s.pointAt > TEAR_PHYSICS.stillMs;
        s.vx = still ? 0 : clamp(s.pvx, -TEAR_PHYSICS.velClamp, TEAR_PHYSICS.velClamp);
        s.vy = still ? 0 : clamp(s.pvy, -TEAR_PHYSICS.velClamp, TEAR_PHYSICS.velClampDown);
        s.spin = clamp(s.vx * 0.004, -6, 6) + TEAR_PHYSICS.dropSpin * s.sign;
        s.age = 0;
        s.phase = "drop";
    } else if (s.phase === "held") {
        s.phase = "return";
    }
}

export function resetTear(s: TearSim, geo: TearGeometry): void {
    const n = geo.bridges.length;
    s.phase = "idle";
    s.pointerId = null;
    s.theta = 0;
    s.thetaV = 0;
    s.sx = 0;
    s.sy = 0;
    s.vx = 0;
    s.vy = 0;
    s.spin = 0;
    s.pvx = 0;
    s.pvy = 0;
    s.fade = 1;
    s.age = 0;
    s.torn = false;
    s.snapped = new Array(n).fill(false);
    s.snapAt = new Array(n).fill(0);
    s.span = new Array(n).fill(0);
}

/** 键盘（Enter/Space）与「减少动态效果」：不演坠落，当场算撕完 */
export function tearInstant(s: TearSim, geo: TearGeometry): void {
    resetTear(s, geo);
    s.torn = true;
}

/**
 * 推进一帧。返回这一帧之后循环还要不要续着跑（`drop` 与 `return` 自己会停，淡尽那帧给 `torn`）。
 * 断裂判据 `2·d·sin(θ/2) + slack > stretch`：`d` 是这条桥到铰点的距离，所以**离铰点越远越先断**，
 * 撕口从远端一路啃向铰点——真纸票就是这个次序。
 */
export function stepTear(s: TearSim, geo: TearGeometry, now: number, dt: number): void {
    const P = TEAR_PHYSICS;
    const limit = rad(P.tearAngle);
    if (s.phase === "held") {
        const count = geo.bridges.length;
        let intact = 0;
        for (let i = 0; i < count; i += 1) if (!s.snapped[i]) intact += 1;
        const follow = P.followMax * (1 - clamp(P.resistance, 0, 0.95) * (count ? intact / count : 0));
        const a = Math.atan2(s.point.y - s.hinge.y, s.point.x - s.hinge.x);
        const want = clamp(wrap(a - s.a0) * s.sign * follow, 0, limit + 0.1);
        s.theta += (want - s.theta) * (1 - Math.exp(-dt / P.tauAngle));
        const away = clamp((s.point.x - s.start.x) * P.awayGain, P.awayLo, P.awayHi);
        const side = clamp((s.point.y - s.start.y) * P.sideGain, P.sideLo, P.sideHi);
        s.sx += (away - s.sx) * (1 - Math.exp(-dt / P.tauSlide));
        s.sy += (side - s.sy) * (1 - Math.exp(-dt / P.tauSlide));
        const slack = Math.hypot(s.sx, s.sy);
        let left = 0;
        geo.bridges.forEach((b, i) => {
            if (s.snapped[i]) return;
            const d = Math.abs(b.mid - s.hingeV);
            if (2 * d * Math.sin(s.theta / 2) + slack > P.stretch || s.theta >= limit) {
                s.snapped[i] = true;
                s.snapAt[i] = now;
                s.bv -= P.bodySnapImpulse / count;
            } else left += 1;
        });
        if (left === 0) {
            s.phase = "free";
            s.bv -= P.bodyCutImpulse;
        }
    } else if (s.phase === "free") {
        // 全断之后手还按着：票根跟着手指走，但只跟到「绕铰点转过后剩下的那点位移」
        const cos = Math.cos(s.theta * s.sign);
        const sin = Math.sin(s.theta * s.sign);
        const gx = s.grab.x - s.hinge.x;
        const gy = s.grab.y - s.hinge.y;
        const wx = s.point.x - s.hinge.x - (gx * cos - gy * sin);
        const wy = s.point.y - s.hinge.y - (gx * sin + gy * cos);
        s.sx += (wx - s.sx) * (1 - Math.exp(-dt / P.tauFreeSlide));
        s.sy += (wy - s.sy) * (1 - Math.exp(-dt / P.tauFreeSlide));
        const hang = limit * P.hangRatio + clamp(s.pvx * P.hangVel * s.sign, -P.hangVelClamp, P.hangVelClamp);
        s.theta += (hang - s.theta) * (1 - Math.exp(-dt / P.tauHang));
    } else if (s.phase === "drop") {
        s.age += dt;
        s.vy += P.gravity * dt;
        s.sx += s.vx * dt;
        s.sy += s.vy * dt;
        s.theta += s.spin * dt;
        if (s.age > P.fadeAfter) {
            s.fade = clamp(1 - (s.age - P.fadeAfter) / P.fadeDur, 0, 1);
        }
        if (s.fade <= 0) {
            s.phase = "idle";
            s.torn = true;
        }
    } else if (s.phase === "return") {
        s.thetaV += (-P.returnK * s.theta - P.returnC * s.thetaV) * dt;
        s.theta += s.thetaV * dt;
        s.sx += (0 - s.sx) * (1 - Math.exp(-dt / P.tauReturn));
        s.sy += (0 - s.sy) * (1 - Math.exp(-dt / P.tauReturn));
        if (
            Math.abs(s.theta) < P.settleTheta &&
            Math.abs(s.thetaV) < P.settleThetaV &&
            Math.hypot(s.sx, s.sy) < P.settlePos
        ) {
            s.theta = 0;
            s.thetaV = 0;
            s.sx = 0;
            s.sy = 0;
            s.phase = "idle";
            // 弹回原位就是把票根拼回去：断裂位不跟着下一趟
            s.snapped.fill(false);
            s.snapAt.fill(0);
        }
    }
    // 主体的回缩：每断一格被拽一下，弹簧自己收平（撕完那一下不拽，只留 150 的余震）
    s.bv += (-P.bodyK * s.bx - P.bodyC * s.bv) * dt;
    s.bx += s.bv * dt;
}

export interface TearParts {
    bodyEl: HTMLElement | null;
    stubEl: HTMLElement | null;
    fibres: SVGPathElement[];
}

/**
 * 把 sim 写成 DOM：票根（转 + 平移 + 淡出）、主体（横向回缩）、断口之间的丝。
 * 丝分两段：没断的那格画「两端各一根、中间垂下去」的活纤维，断了的那格改画「从两端往回缩」的残丝
 * ——一段二次贝塞尔和两条直线，撑得越长丝越细（`fibreW - fibreWK·k`），断完 `retract` 秒内缩没。
 * 返回「还有没有在演的丝」，循环据此决定要不要续帧。
 */
export function paintTear(s: TearSim, geo: TearGeometry, now: number, parts: TearParts): boolean {
    const P = TEAR_PHYSICS;
    if (parts.stubEl) {
        parts.stubEl.style.transform = `translate(${s.sx.toFixed(2)}px, ${s.sy.toFixed(2)}px) rotate(${((s.theta * s.sign * 180) / Math.PI).toFixed(3)}deg)`;
        parts.stubEl.style.opacity = s.fade.toFixed(3);
    }
    if (parts.bodyEl) parts.bodyEl.style.transform = `translateX(${s.bx.toFixed(2)}px)`;

    const cos = Math.cos(s.theta * s.sign);
    const sin = Math.sin(s.theta * s.sign);
    let busy = false;
    geo.bridges.forEach((b, i) => {
        const dx = b.x - s.hinge.x;
        const dy = b.y - s.hinge.y;
        const tx = s.hinge.x + dx * cos - dy * sin + s.sx;
        const ty = s.hinge.y + dx * sin + dy * cos + s.sy;
        const ox = b.x + s.bx;
        const oy = b.y;
        const gx = tx - ox;
        const gy = ty - oy;
        const gap = Math.hypot(gx, gy);
        const near = parts.fibres[i * 2];
        const far = parts.fibres[i * 2 + 1];
        if (!near || !far) return;
        const live = s.phase !== "idle";
        if (!s.snapped[i]) {
            if (!live || gap < P.fibreGapPx) {
                near.style.opacity = "0";
                far.style.opacity = "0";
                return;
            }
            const k = clamp(gap / P.stretch, 0, 1);
            const sag = gap * P.fibreSag;
            const w = (P.fibreW - P.fibreWK * k).toFixed(2);
            const sx = gx / 2;
            const sy = sag + gy / 2;
            near.setAttribute("d", `M${f2(ox)},${f2(oy - P.fibreOffset)}Q${f2(ox + sx)},${f2(oy - P.fibreOffset + sy)} ${f2(tx)},${f2(ty - P.fibreOffset)}`);
            far.setAttribute("d", `M${f2(ox)},${f2(oy + P.fibreOffset)}Q${f2(ox + gx - sx)},${f2(oy + P.fibreOffset + gy - sy)} ${f2(tx)},${f2(ty + P.fibreOffset)}`);
            near.style.strokeWidth = w;
            far.style.strokeWidth = w;
            near.style.opacity = "1";
            far.style.opacity = "1";
            s.span[i] = gap;
            return;
        }
        const t = (now - s.snapAt[i]) / 1000 / P.retract;
        if (!live || t >= 1 || !s.snapAt[i]) {
            near.style.opacity = "0";
            far.style.opacity = "0";
            return;
        }
        busy = true;
        const left = (1 - t) * (1 - t);
        const len = (s.span[i] || P.stretch) * 0.5 * left;
        const ux = gap > 0.01 ? gx / gap : 1;
        const uy = gap > 0.01 ? gy / gap : 0;
        near.setAttribute("d", `M${f2(ox)},${f2(oy)}L${f2(ox + ux * len)},${f2(oy + uy * len)}`);
        far.setAttribute("d", `M${f2(tx)},${f2(ty)}L${f2(tx - ux * len)},${f2(ty - uy * len)}`);
        near.style.strokeWidth = String(P.fibreRetractW);
        far.style.strokeWidth = String(P.fibreRetractW);
        near.style.opacity = left.toFixed(2);
        far.style.opacity = left.toFixed(2);
    });
    return busy;
}

/** 循环这一帧之后还要不要续：状态机没静止、主体还在回缩、丝还在缩，三者有一个就得再来一帧 */
export function tearBusy(s: TearSim, painting: boolean): boolean {
    return (
        s.phase !== "idle" ||
        Math.abs(s.bx) > 0.02 ||
        Math.abs(s.bv) > 0.5 ||
        painting
    );
}

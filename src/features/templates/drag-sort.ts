/**
 * 模板卡排序的手写运动件（判据 + 积分器），页文件只管 DOM。
 *
 * 为什么不用 motion 的 `layout`：那条弹簧只在投影节点**创建时**读一次目标，而这里的拖动是
 * 「每帧换一个落点」——跟随体必须由 rAF 逐帧积分，声明式 layout 演不出来。增删换位也走同一套：
 * 行的位置一律等于「自己的弹簧顶 − 布局顶」写成的 transform，布局那份永远是真的，transform 收到 0
 * 就是它在流里的家，所以松手那一帧不需要任何补间也不会重排一次。
 *
 * 三条口径别写回去：
 *  1. **落位索引只认原始输入**（指针算出来的顶），不认磁吸混合后的位置。混合位置再喂回索引，
 *     吸附就会自己拖着索引连锁跳格。
 *  2. **范围边缘不能突跳**：权重走 smoothstep，值与斜率同时归零，所以出range 那一帧没有台阶。
 *  3. **进入与退出用不同的线**（`band` 半带滞回）：格心是整数除法、边界两侧各贴一次就翻，交界会抖。
 *  4. **拖动途中用弹簧、松手吸附用时间补间**（`landEase`）：弹簧是有质量的振子，ζ<1 就一定过冲、
 *     ζ≥1 就一定是「越拖越慢地贴过去」；他要的「猛的吸进去且不反弹」只有 `u²` 给得起——末速最大、
 *     终点精确、结构上无越线。两条各自收口，别拿一条去凑另一条的手感。
 */

export const SORT = {
    /** 让位的行：ζ≈0.71，跟到位、几乎不过冲（过冲＝行程×比例，95px 的行程经不起软） */
    rowStiffness: 340,
    rowDamping: 26,
    /** 抬起层的跟随（只在拖动途中生效，松手之后走 `landMs` 那条补间）：ζ≈0.53 带着磁吸那点迟滞 */
    followStiffness: 900,
    followDamping: 32,
    /**
     * 松手吸附的时长：位移走 `landEase`（u²），起步慢、撞进格子那一瞬最快，**结构上没有过冲**。
     * 距离不换时长——行程越长，末速越大，「猛」的那一下就是这么来的。
     */
    landMs: 140,
    /**
     * 抬起层的摇摆：**一条正弦、来回过零**，所以它不是「固定左高右低」那种死倾角。
     * `swayDeg` 是满幅（按 540px 卡宽读：1.1° ≈ 边缘垂 5px），`swayMs` 是一个来回的周期。
     * `swayFullPxS` 是走到满幅所需的指针速度，`swayEaseMs` 是包络的低通时间常数——
     * 捏着不动时包络自己收到 0 ⇒ 摆会平息，不会原地打圈。
     */
    swayDeg: 1.1,
    swayMs: 900,
    swayFullPxS: 260,
    swayEaseMs: 220,
    /** 磁吸：半径与最强权重（0 在半径外，`weight` 在格心） */
    magnetRadius: 90,
    magnetWeight: 0.55,
    /** 换格的滞后带宽度：进出各占一半 */
    band: 10,
    /** 错峰：离落点越远的行起步越晚，档数封顶（否则整列同帧起步读成「一坨」） */
    staggerMs: 12,
    staggerSteps: 6,
    /** 超过这个位移才算「要拖」：轻点把手不该把卡抬起来 */
    liftAfterPx: 3,
    /** 抬起层离窗口上下沿各留这么多：指针甩出窗口之后那几帧，卡就钉在边界上不再跟手 */
    edgeInset: 8,
    /** 行收敛判定：到位就钉死在目标上，别把 0.3px 的 residue 留在 transform 里 */
    rowSettlePx: 0.4,
    rowSettleV: 4,
} as const;

/** 一帧最多积这么多秒：标签页回前台、断点调试都不该让弹簧拿几秒的 dt 冲出去 */
export const MAX_DT = 1 / 30;

/** 行/抬起层共用的弹簧状态；`y` 一律是「容器（或视口）坐标里的顶」 */
export interface Spring {
    y: number;
    v: number;
}

/** 半隐式欧拉：先更新速度再更新位置，能量单调，比显式欧拉稳 */
export function integrate(s: Spring, target: number, k: number, c: number, dt: number): void {
    const a = (-k * (s.y - target) - c * s.v) / 1;
    s.v += a * dt;
    s.y += s.v * dt;
}

/** 已经到位（并把残余速度收掉）⇒ 可以把 y 钉在 target 上 */
export function settled(s: Spring, target: number, px: number, v: number): boolean {
    return Math.abs(s.y - target) < px && Math.abs(s.v) < v;
}

/**
 * 指针中心落在第几格：`rel` 是中心相对列表盒顶的距离，`gap` 是行距。
 * 加半条行距是为了把格线放到相邻两格的分界中央——上下两个方向共用同一条线，没有半格偏置。
 * 上限是 `maxSlot`（= 其余卡的条数，也就是最后一个落位格）。
 */
export function natIndex(rel: number, pitch: number, gap: number, maxSlot: number): number {
    if (pitch <= 0) return 0;
    return Math.max(0, Math.min(maxSlot, Math.floor((rel + gap / 2) / pitch)));
}

/**
 * 带滞回的换格：一步跨过两格以上（快速穿越）直接跟上；否则必须**越过边界再多半个带**才算换格，
 * 回到带内保持原位。交界线上来回蹭不再翻（验收：`band` 之内抖动 ⇒ 换格 0 次）。
 */
export function stepIndex(
    idx: number,
    rel: number,
    nat: number,
    pitch: number,
    band: number,
    maxSlot: number
): number {
    if (nat >= idx + 2 || nat <= idx - 2) return nat;
    if (nat > idx && rel >= (idx + 1) * pitch + band / 2) return nat;
    if (nat < idx && rel <= idx * pitch - band / 2) return nat;
    return Math.max(0, Math.min(maxSlot, idx));
}

/**
 * 磁吸混合：`rawTop` 是指针算出来的原始顶，`slotTop` 是当前落位格的目标顶。
 * 返回的是**抬起层这一帧该去的位置**——它不是索引判据（见文件头第 1 条），也不是提交判据。
 * 权重 `w = weight · smoothstep(1 − |d|/radius)`：范围外恒为 0，边界处值与斜率同时归零。
 */
export function magnet(
    rawTop: number,
    slotTop: number,
    radius: number,
    weight: number
): { top: number; w: number; d: number } {
    const d = rawTop - slotTop;
    const ad = Math.abs(d);
    if (radius <= 0 || ad >= radius) return { top: rawTop, w: 0, d };
    const t = 1 - ad / radius;
    const w = weight * t * t * (3 - 2 * t);
    return { top: rawTop + w * (slotTop - rawTop), w, d };
}

/**
 * 抬起层不许被甩出窗口：把「指针算出来的原始顶」夹在窗口上下沿各留 `inset` 的那一段里。
 * **夹在判据之前**（索引与磁吸都吃夹过的值），所以越界那几帧位置和落点一起冻住——
 * 只夹渲染的话会出现「卡停在边界、空槽还在往里挪」那种对不上。窗口比一张卡还矮时钉在上沿。
 */
export function clampTop(rawTop: number, height: number, winH: number, inset: number): number {
    const max = winH - inset - height;
    if (max < inset) return inset;
    return Math.max(inset, Math.min(max, rawTop));
}

/**
 * 落位曲线（松手吸附）：`u∈[0,1]` 是时间进度，返回位移进度。`u²` 单调、恒 ≤1，
 * 所以「越过格子」在结构上不可能发生——这一条替代了弹簧（弹簧只要 ζ<1 必然过冲）。
 * 速度是它的导数 `2u/dur`：起步 0、到达时最大，读起来就是「被猛吸进格子」而不是「弹一下停住」。
 */
export function landEase(u: number): number {
    const t = u <= 0 ? 0 : u >= 1 ? 1 : u;
    return t * t;
}

/**
 * 抬起层的摇摆角（度，CSS `rotate`：正＝顺时针＝右缘下垂＝左高右低，负＝反过来）。
 * 之所以是正弦而不是「速度换角度」：后者在纯竖着拖时速度恒 0 ⇒ 卡永远水平，而摆动那一下正是他要的
 * 「卡是被拿在手里的」的信号；正弦自己会过零，所以两个方向轮着来，不会定在一边。
 * `env` 是活跃度包络（0..1）：捏着不动它自己收到 0，摆就平息——不留一个永动的抖动。
 */
export function swayAngle(phase: number, ampDeg: number, env: number): number {
    return ampDeg * env * Math.sin(phase);
}

/** 包络的目标值：指针速度到 `fullPxS` 就吃满幅，静止归零 */
export function swayEnv(speedPxS: number, fullPxS: number): number {
    if (fullPxS <= 0) return 1;
    return Math.max(0, Math.min(1, speedPxS / fullPxS));
}

/**
 * 松手时的落点判定：指针中心还在列表范围内（上下各多让半张卡）才算「要放在这里」。
 * 出界与取消一律交回原序 ⇒ 结构上不可能把一次无效的拖拽写成提交。
 */
export function inRange(relCenter: number, height: number, gridHeight: number): boolean {
    return relCenter >= -height / 2 && relCenter <= gridHeight + height / 2;
}

/** 显示序列里第 `index` 张卡的家位（容器坐标的顶；列表盒没有内边距，所以格网原点就是 0） */
export function homeOf(index: number, pitch: number): number {
    return index * pitch;
}

/** 显示序列里第 `i` 张其余卡的顶格位置：空槽在它自己的那一格里，它和后面的都让开 */
export function slotOf(i: number, idx: number, pitch: number): number {
    return homeOf(i >= idx ? i + 1 : i, pitch);
}

/** 把 `others` 在 `idx` 处插入拖动项，得到落位后的完整顺序 */
export function spliceOrder(others: string[], id: string, idx: number): string[] {
    const at = Math.max(0, Math.min(others.length, idx));
    return [...others.slice(0, at), id, ...others.slice(at)];
}

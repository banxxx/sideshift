/**
 * 密排卡名单：卡片入场、悬浮 3D、点击波浪涟漪，以及「收起 ≤4 行 / 展开看全部」那一档高度。
 *
 * 四个硬约束都是撞出来的，改之前先读完：
 *
 *  1. **两条 transform 不能落在同一个元素上**。入场（motion 写 y/opacity）跑在外层 cell，
 *     倾斜（本组件用 inline style 写 transform）跑在内层按钮上：WAAPI 的 `fill:"both"` 会永久
 *     压住后来的 inline transform，两条挤一个节点上就是互相顶掉（样片里入场动画把视差压死过）。
 *     涟漪因此走 `scale` / `borderColor` 这两个**独立属性**——也正因为如此它**不能用 `filter`
 *     做"亮一下"**：`filter` 创建层叠上下文，会把 `preserve-3d` 当场压平。
 *  2. **`will-change` 与子层的 `translateZ` 只在悬停那一张上挂**。常驻的话 N 张卡 = N 个合成层
 *     与 N 个 3D 上下文，撞「日志量级=前端性能预算」那条。
 *  3. **涟漪的半径参照是「看得见的这一档」（壳），不是整条名单**：默认只露 4 行时按全高算半径，
 *     会把大半时间花在屏外那些行上。半径取"被点这张 → 可见区四角"的最远一个，点哪儿都扫得到全片。
 *  4. 档高**量真 DOM**（按每张卡的 `offsetTop` 分行），不写公式：昵称字号跟卡尺走，
 *     公式算出来的行高会在换度量时差几像素。transform 不参与 offsetTop/offsetHeight，
 *     所以入场正演着也量得准。被裁住的行不演涟漪（名单很长时省掉一屏无效动画）。
 *
 * `overflow:hidden` 是常驻的，这里**不能**照公共 `Collapse` 的「演完撤 overflow」办：
 * 收起档的内容本来就比壳高，撤了当场漏出后面几行。代价说清楚——贴边那行卡悬浮时的 3D 翘起
 * 与投影会被这条边切到，所以档高留了 `BREATHE` 的呼吸量，别去动 overflow。
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { FoldBtn, Panel, PanelHead } from "@/components/ui";
import { COLLAPSE, waveFrontDurMs, waveFrontTime, WAVE_FRONT_EASE } from "@/lib/page-motion";
import { RISE } from "@/lib/springs";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { initialsOf, type Contributor } from "./contributors";

/** 以下数值全部取自 `.scratch/about-proto.html` 挑定的那一档（样片参数快照），改这里就是改观感 */
const S = 30;
/** 卡与卡的行列间距 */
const GAP = 8;
/** 收起档最多几行；行数不足则有几行显示几行 */
const MAX_ROWS = 4;
/** 档高的呼吸量：给贴边那行卡的翘起与投影留出的余量（px） */
const BREATHE = 10;
/** 悬浮：倾角上限（度）、凸起（px）、透视距离（px）、头像与昵称各自再浮的行程（px）、离开复位时长（ms） */
const TILT = { deg: 12, lift: 14, pd: 800, zAvatar: 16, zName: 16, backMs: 620 };
/** 涟漪：强度（%）、波速（px/s，口径＝等速主体段的速度）、单张脉冲时长（ms）、点击锁（ms） */
const RIP = { strength: 18, speed: 250, durMs: 520, lockMs: 420 };
/** 入场：行程（px）与错峰（秒），错峰总时长压进 budget（否则 240 人要排到 2 秒开外） */
const ENTRY = { rise: 8, stagger: 0.008, budget: 0.7 };

/** `#rrggbb` → rgba：涟漪的描边透明度按距离连续变，而 `--accent` 在 App.css 里就是十六进制字面量 */
function withAlpha(hex: string, a: number): string {
    const h = hex.replace("#", "").trim();
    const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
    const n = parseInt(full, 16);
    if (Number.isNaN(n)) return hex;
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}

function AckCard({ p, delay }: { p: Contributor; delay: number }) {
    const btn = useRef<HTMLButtonElement>(null);
    /** 正在演的那条复位动画：只握它自己的句柄——`getAnimations()` 一把 cancel 会把正扫过来的涟漪掐断 */
    const back = useRef<Animation | null>(null);
    const reduced = useReducedMotion();

    const leave = useCallback(() => {
        const el = btn.current;
        if (!el) return;
        const from = el.style.transform;
        el.style.transform = "";
        back.current?.cancel();
        back.current = null;
        if (reduced || !from) return;
        // 复位走波浪第二段的刹车曲线：起始斜率 2.0 ⇒ 一撤就动，末端斜率 0.013 ⇒ 最后那点是爬回去的。
        // 单卡只有十几度/十几 px，不叠"等速主体段"——那一段是给横跨整片的大行程保匀速观感用的
        const a = el.animate([{ transform: from }, { transform: "none" }], {
            duration: TILT.backMs,
            easing: WAVE_FRONT_EASE,
            fill: "both",
        });
        back.current = a;
        a.onfinish = () => {
            if (back.current === a) back.current = null;
            // 必须撤：fill:"both" 留着会永久压住下一次的 inline transform
            a.cancel();
            el.style.transform = "";
        };
    }, [reduced]);

    // 卸载（切页、换语言重挂）时收掉没演完的那条
    useEffect(() => () => back.current?.cancel(), []);

    return (
        <motion.div
            className="inline-flex min-w-0"
            initial={{ opacity: 0, y: ENTRY.rise }}
            animate={{ opacity: 1, y: 0, transition: { ...RISE, delay } }}
        >
            <button
                ref={btn}
                type="button"
                data-ack-card
                aria-label={p.name}
                onPointerMove={(e) => {
                    const el = btn.current;
                    if (!el || reduced) return;
                    const r = el.getBoundingClientRect();
                    const dx = (e.clientX - r.left) / r.width - 0.5;
                    const dy = (e.clientY - r.top) / r.height - 0.5;
                    // 右移⇒右边往后（rotateY 取正）；下移同理取负
                    el.style.transform = `perspective(${TILT.pd}px) rotateX(${(-dy * TILT.deg * 2).toFixed(2)}deg) rotateY(${(dx * TILT.deg * 2).toFixed(2)}deg) translateZ(${TILT.lift}px)`;
                    el.style.setProperty("--ack-gx", `${((dx + 0.5) * 100).toFixed(1)}%`);
                    el.style.setProperty("--ack-gy", `${((dy + 0.5) * 100).toFixed(1)}%`);
                }}
                onPointerLeave={leave}
                className={cn(
                    "group/ack relative flex min-w-0 items-center rounded-md border border-stroke bg-surface",
                    "transition-[border-color,box-shadow] duration-200 hover:border-(--ack-border)",
                    "hover:shadow-[0_10px_22px_-8px_var(--ack-glow),0_2px_6px_rgba(0,0,0,0.1)]",
                    // 只在悬停那一张上开合成层
                    !reduced && "hover:will-change-transform"
                )}
                style={{
                    gap: S * 0.28,
                    padding: `${S * 0.22}px ${S * 0.42}px ${S * 0.22}px ${S * 0.22}px`,
                    transformStyle: "preserve-3d",
                }}
            >
                {p.avatar ? (
                    <img
                        src={p.avatar}
                        alt=""
                        aria-hidden
                        draggable={false}
                        className="shrink-0 rounded-[4px] object-cover transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
                        style={{ width: S, height: S }}
                    />
                ) : (
                    <span
                        aria-hidden
                        className={cn(
                            "flex shrink-0 items-center justify-center rounded-[4px] bg-surface-2",
                            "font-mono text-[12px] font-semibold uppercase text-text-2",
                            "transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]"
                        )}
                        style={{ width: S, height: S }}
                    >
                        {initialsOf(p.name)}
                    </span>
                )}
                <span className="max-w-[150px] truncate text-[12.5px] leading-[1.25] font-medium text-text-1 transition-transform duration-200 group-hover/ack:[transform:translateZ(16px)]">
                    {p.name}
                </span>
                {/* 高光跟指针走。这里不开 overflow:hidden——它会把 preserve-3d 的分层浮起压平 */}
                <span
                    aria-hidden
                    className="pointer-events-none absolute inset-0 rounded-[inherit] opacity-0 transition-opacity duration-200 group-hover/ack:opacity-100"
                    style={{
                        background:
                            "radial-gradient(78px 78px at var(--ack-gx,50%) var(--ack-gy,50%), rgba(255,255,255,.2), rgba(255,255,255,0) 62%)",
                    }}
                />
            </button>
        </motion.div>
    );
}

export function AckWall({ people }: { people: Contributor[] }) {
    const t = useT();
    const shell = useRef<HTMLDivElement>(null);
    const wall = useRef<HTMLDivElement>(null);
    const gate = useRef(0);
    const [open, setOpen] = useState(false);
    const [band, setBand] = useState({ closed: 0, full: 0, expandable: false });
    const reduced = useReducedMotion();

    /** 行按 `offsetTop` 分组：同一次换行落下来的卡 top 相同，差 1px 以内算测量误差 */
    const measure = useCallback(() => {
        const node = wall.current;
        if (!node) return;
        const rows: { top: number; h: number }[] = [];
        for (const cell of Array.from(node.children)) {
            const el = cell as HTMLElement;
            const last = rows[rows.length - 1];
            if (!last || Math.abs(last.top - el.offsetTop) > 1) {
                rows.push({ top: el.offsetTop, h: el.offsetHeight });
            } else {
                last.h = Math.max(last.h, el.offsetHeight);
            }
        }
        const k = Math.min(rows.length, MAX_ROWS);
        const next = {
            closed: k ? rows[k - 1].top + rows[k - 1].h + BREATHE : 0,
            full: node.offsetHeight + BREATHE,
            expandable: rows.length > MAX_ROWS,
        };
        setBand((prev) =>
            prev.closed === next.closed && prev.full === next.full && prev.expandable === next.expandable
                ? prev
                : next
        );
    }, []);

    useLayoutEffect(measure, [measure, people]);
    // 窗口宽度一变行的分布就变（逢改必流体），档高与「有没有展开钮」都得跟着重算
    useEffect(() => {
        const node = wall.current;
        if (!node || typeof ResizeObserver === "undefined") return;
        const ro = new ResizeObserver(measure);
        ro.observe(node);
        return () => ro.disconnect();
    }, [measure]);

    // 人数或宽度变到"收起档就装得下"时退回收起态：否则符号停在朝上的那一面却没有可收的东西
    useEffect(() => {
        if (!band.expandable && open) setOpen(false);
    }, [band.expandable, open]);

    /** 点一张卡 ⇒ 波前从那儿扫过可见的这一档，半径内的邻居各脉冲一下 */
    const ripple = useCallback(
        (src: HTMLElement) => {
            if (reduced || !RIP.strength || !wall.current) return;
            // 前沿触发的闸门：锁窗口内的后续点击直接吞掉。不排队——两波浪叠在同一批卡上互相盖，
            // 比"这一下没反应"更难读
            const now = performance.now();
            if (now < gate.current) return;
            gate.current = now + RIP.lockMs;

            const o = src.getBoundingClientRect();
            const ox = o.left + o.width / 2;
            const oy = o.top + o.height / 2;
            const b = (shell.current ?? src).getBoundingClientRect();
            const R = Math.round(
                Math.max(
                    Math.hypot(ox - b.left, oy - b.top),
                    Math.hypot(b.right - ox, oy - b.top),
                    Math.hypot(ox - b.left, oy - b.bottom),
                    Math.hypot(b.right - ox, oy - b.bottom)
                )
            );
            const T = waveFrontDurMs(R, RIP.speed);
            const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent");

            for (const el of Array.from(wall.current.querySelectorAll<HTMLElement>("[data-ack-card]"))) {
                if (el === src) continue;
                const r = el.getBoundingClientRect();
                // 被裁住的那几行不演：屏上看不见的东西没有观众
                if (r.bottom < b.top || r.top > b.bottom) continue;
                const dx = r.left + r.width / 2 - ox;
                const dy = r.top + r.height / 2 - oy;
                const d = Math.hypot(dx, dy) || 1;
                if (d > R) continue;
                // 幅度按线性衰减（半径就是"这一圈以外不理会"）；曲线只管**什么时候**扫到
                const fall = 1 - d / R;
                const sc = (RIP.strength / 100) * (0.3 + 0.7 * fall);
                el.animate(
                    [
                        { scale: "1", borderColor: getComputedStyle(el).borderTopColor, easing: "cubic-bezier(.2,.8,.2,1)" },
                        {
                            scale: String(1 + sc),
                            borderColor: withAlpha(accent, 0.2 + 0.6 * fall),
                            offset: 0.34,
                            easing: WAVE_FRONT_EASE,
                        },
                        { scale: "1", borderColor: getComputedStyle(el).borderTopColor },
                    ],
                    { duration: RIP.durMs, delay: waveFrontTime(d / R) * T, fill: "none" }
                );
            }
            // 波源那张先被按下去一点、再慢慢回弹：没有圆环，这一下就是"点在这儿"的全部线索
            src.animate(
                [
                    { scale: "1", easing: "cubic-bezier(.2,.8,.2,1)" },
                    { scale: String(1 - (RIP.strength / 100) * 0.6), offset: 0.24, easing: WAVE_FRONT_EASE },
                    { scale: "1" },
                ],
                { duration: 300, fill: "none" }
            );
        },
        [reduced]
    );

    const height = band.expandable && open ? band.full : band.closed;

    return (
        <Panel gap={10}>
            <PanelHead
                title={t("about.acknowledgements", "鸣谢名单")}
                right={
                    band.expandable ? (
                        <FoldBtn
                            open={open}
                            label={
                                open
                                    ? t("about.collapse-ack", "收起鸣谢名单")
                                    : t("about.expand-ack", "展开鸣谢名单")
                            }
                            onClick={() => setOpen((v) => !v)}
                        />
                    ) : undefined
                }
            />
            <motion.div
                ref={shell}
                initial={false}
                animate={{ height }}
                transition={COLLAPSE}
                // 常驻裁剪，理由见文件头第 4 条
                className="overflow-hidden"
                onClick={(e) => {
                    const card = (e.target as HTMLElement).closest("[data-ack-card]");
                    if (card) ripple(card as HTMLElement);
                }}
            >
                <div ref={wall} className="relative flex flex-wrap items-start" style={{ gap: GAP }}>
                    {people.map((p, i) => (
                        <AckCard key={p.id} p={p} delay={i * Math.min(ENTRY.stagger, ENTRY.budget / Math.max(1, people.length))} />
                    ))}
                </div>
            </motion.div>
        </Panel>
    );
}

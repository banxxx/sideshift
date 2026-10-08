/**
 * 一张「可以撕」的卡：左半是主体、右半 80px 是票根，中间一条孔列。捏住票根绕孔列的一端转出角度 ⇒ 桥一段段断、丝一根根缩，全断后票根坠出屏，再通知父层收高整行。判据与积分在 tear-ticket.ts，这里只有 DOM 与事件。
 * 硬约束一：宽度由父层量了传进来（`clip-path: path()` 吃 px，缩放适配会让撕口和卡宽脱钩）。
 * 硬约束二：`clip-path` 连 box-shadow 一起裁掉 ⇒ 投影挂在裁层的**父级**（`filter: drop-shadow`，跟着剪影走）。
 * 硬约束三：票根那层是 `inset-0` 的全尺寸盒子，容器本身不吃命中，只有被裁剩下的那块面吃——否则主体上的按钮全被它盖掉。`pointer-events` 是**继承**的，所以那块面必须自己写回 `pointer-events-auto`，光靠容器置空等于整张票根都捏不到（`elementFromPoint` 实测：命中落到主体那层的裁外余盒上）。
 */
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useReducedMotion } from "motion/react";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import {
    beginTear,
    buildTearGeometry,
    createTearSim,
    feedTear,
    paintTear,
    releaseTear,
    resetTear,
    stepTear,
    tearBusy,
    tearInstant,
    TEAR,
    type TearGeometry,
} from "./tear-ticket";

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** 在飞的撕票数：>0 时内容区横向溢出是票根甩出去的，滚动条要按住（见 `raise`） */
let tearsInFlight = 0;

export function TearTicket({
    width,
    body,
    stub,
    lifted,
    disabled,
    frost = "card-frost",
    onTear,
}: {
    /** 卡的实测宽：变了要重建孔列几何（流体宽的代价就这一条） */
    width: number;
    /** 撕口左边的整张卡（含它自己的交互） */
    body: ReactNode;
    /** 票根上的内容（垃圾桶） */
    stub: ReactNode;
    /** 这张已经交给抬起层代管：整块藏起来，票根也不吃手势 */
    lifted?: boolean;
    /** 拖排进行中：票根不吃手势（一只手抓着把手、另一只手去撕，落点谁都说不上） */
    disabled?: boolean;
    /** 两枚面共用的磨砂档：抬起层要浮在列表之上，那里传 `modal-frost` */
    frost?: string;
    /** 票根坠尽那一刻（父层据此收高整行并写盘）。抬起层不演撕，可以不传 */
    onTear?: () => void;
}) {
    const t = useT();
    const reduced = useReducedMotion() === true;
    const rootRef = useRef<HTMLDivElement | null>(null);
    const bodyRef = useRef<HTMLDivElement | null>(null);
    const stubRef = useRef<HTMLDivElement | null>(null);
    const svgRef = useRef<SVGSVGElement | null>(null);
    const fibresRef = useRef<SVGPathElement[]>([]);
    const simRef = useRef(createTearSim());
    const geoRef = useRef<TearGeometry | null>(null);
    const onTearRef = useRef(onTear);
    onTearRef.current = onTear;
    const rafRef = useRef(0);
    const lastRef = useRef(0);
    const usedRef = useRef(false);
    /** 抬起时写过的宿主（行 + 内容区滚动容器）；null = 这趟没抬着 */
    const raisedRef = useRef<{ row: HTMLElement; scroller: HTMLElement | null } | null>(null);
    const [geo, setGeo] = useState<TearGeometry | null>(null);

    /**
     * 几何按「传进来的宽 + 量到的高」重建。高从 DOM 量（卡高由内容给，别写死 79），
     * 撕完之后不再重算：父层收高正逐帧改这一层的 `height`，跟着量会把裁路径一起压扁。
     */
    useLayoutEffect(() => {
        const el = rootRef.current;
        if (!el || usedRef.current || width < 40) return;
        const h = el.offsetHeight;
        const prev = geoRef.current;
        if (prev && Math.abs(prev.width - width) < 0.5 && Math.abs(prev.height - h) < 0.5) return;
        const next = buildTearGeometry(width, h);
        geoRef.current = next;
        setGeo(next);
    }, [width]);

    /** 丝的路径节点按桥数成对建（每条桥两根：近端一根、远端一根），几何一变就要重取 */
    useLayoutEffect(() => {
        fibresRef.current = svgRef.current
            ? Array.from(svgRef.current.querySelectorAll<SVGPathElement>("path"))
            : [];
    }, [geo]);

    const paintNow = (now: number) => {
        const g = geoRef.current;
        if (!g) return false;
        return paintTear(simRef.current, g, now, {
            bodyEl: bodyRef.current,
            stubEl: stubRef.current,
            fibres: fibresRef.current,
        });
    };

    const stop = () => {
        if (rafRef.current) cancelAnimationFrame(rafRef.current);
        rafRef.current = 0;
        lastRef.current = 0;
    };

    const frame = (now: number) => {
        rafRef.current = 0;
        const g = geoRef.current;
        if (!g) return;
        const s = simRef.current;
        const dt = clamp((now - lastRef.current) / 1000, 0.001, 1 / 30);
        lastRef.current = now;
        // 减少动态效果：不演那一下坠落，断到位就算撕完（手势本身照旧，不会退化成「点一下就删」）
        if (reduced && s.phase === "drop") {
            s.phase = "idle";
            s.torn = true;
        }
        stepTear(s, g, now, dt);
        const retracting = paintNow(now);
        if (s.torn) {
            s.torn = false;
            usedRef.current = true;
            if (stubRef.current) stubRef.current.style.visibility = "hidden";
            raise(false);
            onTearRef.current?.();
            return;
        }
        if (tearBusy(s, retracting)) {
            rafRef.current = requestAnimationFrame(frame);
            return;
        }
        // 静止收尾：回缩那一下的残余直接抹平，别把 0.01px 留在 transform 里
        s.bx = 0;
        s.bv = 0;
        paintNow(now);
        lastRef.current = 0;
        raise(false);
    };

    const kick = () => {
        if (rafRef.current) return;
        lastRef.current = performance.now();
        rafRef.current = requestAnimationFrame(frame);
    };

    /**
     * 撕的整个过程要把这一行抬到兄弟行之上（票根是往下坠的，不抬就被下一张卡盖住）。
     * 行那一层带 transform ⇒ 自己就是个层叠上下文，所以只抬它的 `z-index` 就够；
     * 抬到 10：压得住同场的行，又不会高过页头纱那一档（20）。
     *
     * 同一趟还要把内容区的**横向滚动条**按住：票根往右甩最多顶出 195px 溢出（无头实测
     * `scrollWidth/clientWidth` 1685/1490），不压住撕的过程中底部会闪出一条横滚动条。
     * 计数收口——两张票一前一后在下坠时，先落地的那张不能把另一张还压着的遮挡撤掉。
     * 还原走抬起那一刻记下的宿主，不现找：卸载那一刻 `rootRef` 可能已经没了。
     */
    const raise = (on: boolean) => {
        if (on) {
            if (raisedRef.current) return;
            const root = rootRef.current;
            const row = root?.closest<HTMLElement>("[data-card-id]");
            if (!row) return;
            const scroller = root?.closest<HTMLElement>(".page-scroll") ?? null;
            row.style.zIndex = "10";
            if (scroller) {
                tearsInFlight += 1;
                scroller.style.overflowX = "clip";
            }
            raisedRef.current = { row, scroller };
            return;
        }
        const held = raisedRef.current;
        if (!held) return;
        raisedRef.current = null;
        held.row.style.zIndex = "";
        if (held.scroller) {
            tearsInFlight = Math.max(0, tearsInFlight - 1);
            if (tearsInFlight === 0) held.scroller.style.overflowX = "";
        }
    };

    const local = (e: { clientX: number; clientY: number }) => {
        const r = rootRef.current!.getBoundingClientRect();
        return { x: e.clientX - r.left, y: e.clientY - r.top };
    };

    const blocked = () => usedRef.current || disabled === true || lifted === true;

    const onPointerDown = (e: React.PointerEvent<HTMLElement>) => {
        const s = simRef.current;
        if (blocked() || e.button !== 0 || s.pointerId !== null || s.phase === "drop") return;
        const g = geoRef.current;
        if (!g) return;
        e.preventDefault();
        try {
            e.currentTarget.setPointerCapture(e.pointerId);
        } catch {
            // 拿不到捕获照样能撕，只是指针甩出卡片那几帧收不到 move
        }
        const p = local(e);
        beginTear(s, g, p, performance.now());
        // 认领必须排在 beginTear 之后：完好那一路它会走 resetTear，而 resetTear 会清掉 pointerId，
        // 先写就被擦干净，之后每一条 move 都过不了认领闸——整张票根捏不住。
        s.pointerId = e.pointerId;
        // 转轴读 sim 里那一个（不是现算）：回位演到一半再按下时铰点不换头，跟着写的原点才对得上
        if (stubRef.current) {
            stubRef.current.style.transformOrigin = `${s.hinge.x}px ${s.hinge.y}px`;
        }
        raise(true);
        kick();
    };

    const onPointerMove = (e: React.PointerEvent<HTMLElement>) => {
        const s = simRef.current;
        if (s.pointerId !== e.pointerId || s.phase === "idle") return;
        feedTear(s, local(e), performance.now());
        kick();
    };

    const end = (e: React.PointerEvent<HTMLElement>) => {
        const s = simRef.current;
        if (s.pointerId !== e.pointerId) return;
        s.pointerId = null;
        releaseTear(s, performance.now());
        kick();
    };

    const onKeyDown = (e: React.KeyboardEvent<HTMLElement>) => {
        if (blocked() || (e.key !== "Enter" && e.key !== " ")) return;
        e.preventDefault();
        // 长按 Enter 会连续 repeat：只认第一下，不然一次按键能删好几张
        if (e.repeat) return;
        const g = geoRef.current;
        if (!g) return;
        stop();
        tearInstant(simRef.current, g);
        if (stubRef.current) stubRef.current.style.visibility = "hidden";
        usedRef.current = true;
        onTearRef.current?.();
    };

    // 卸掉时（删除、换页、上一趟还在飞）把循环、抬起的那一格 z-index 和按住的滚动条一起收干净
    useLayoutEffect(() => {
        return () => {
            stop();
            raise(false);
        };
    }, []);

    // 没撕完就整张卸掉（例如切页）：状态归零，别把「断到一半」带到下一趟
    useLayoutEffect(() => {
        if (!geo) return;
        const s = simRef.current;
        if (s.phase !== "idle" && s.pointerId === null) {
            resetTear(s, geo);
            paintNow(performance.now());
        }
    }, [geo]);

    return (
        <div ref={rootRef} data-tear="" className={cn("relative select-none", lifted && "invisible")}>
            {/* 主体：在流里（卡高由它撑），左半的撕口归它的裁路径 */}
            <div ref={bodyRef} className="relative">
                {/*
                  裁的只有「面」：`clip-path` 连整棵子树一起裁，卡里的气泡长出卡外那几像素会被切掉
                  （实测 5.5px）。内容另起一层不裁 ⇒ 撕的时候主体只往左回缩（`bodyCutImpulse` 那一弹，
                  量级 ~10px），而卡的内边距是 20px，露不出剪影。
                */}
                <div className="absolute inset-0" style={{ filter: "var(--tear-shadow)" }}>
                    <div
                        className={cn(frost, "h-full rounded-[12px]")}
                        style={{ clipPath: geo ? `path('${geo.body}')` : undefined }}
                    />
                </div>
                <div className="relative">{body}</div>
            </div>

            {/* 丝：画在主体与票根之间，坐标就是卡的 px 坐标（不给 viewBox，给了会跟着收高一起缩） */}
            <svg
                ref={svgRef}
                aria-hidden="true"
                className="pointer-events-none absolute inset-0 h-full w-full overflow-visible"
            >
                {Array.from({ length: geo?.bridges.length ?? 0 }, (_, i) => (
                    <g key={i}>
                        <path fill="none" stroke="var(--tear-fibre)" strokeLinecap="round" opacity={0} />
                        <path fill="none" stroke="var(--tear-fibre)" strokeLinecap="round" opacity={0} />
                    </g>
                ))}
            </svg>

            {/* 票根：整层是变换载体（转 + 平移 + 淡出），命中只给裁剩下的那块面 */}
            <div ref={stubRef} className="pointer-events-none absolute inset-0 will-change-transform">
                <div className="absolute inset-0" style={{ filter: "var(--tear-shadow)" }}>
                    <div
                        role="button"
                        tabIndex={blocked() ? -1 : 0}
                        aria-label={t("templates.tear-stub", "撕下票根以删除这张模板")}
                        className={cn(
                            frost,
                            "absolute inset-0 touch-none rounded-[12px] outline-none",
                            // 抬起层那张也走这条路：`blocked()` 时连命中都不给，
                            // 不然 `pointer-events-auto` 会盖过抬起层根的 `pointer-events-none`
                            !blocked() && "pointer-events-auto cursor-grab active:cursor-grabbing"
                        )}
                        style={{ clipPath: geo ? `path('${geo.stub}')` : undefined }}
                        onPointerDown={onPointerDown}
                        onPointerMove={onPointerMove}
                        onPointerUp={end}
                        onPointerCancel={end}
                        onLostPointerCapture={end}
                        onKeyDown={onKeyDown}
                    >
                        <span
                            className="absolute inset-y-0 right-0 flex items-center justify-center text-text-3"
                            style={{ width: TEAR.stubSize }}
                        >
                            {stub}
                        </span>
                    </div>
                </div>
            </div>
        </div>
    );
}

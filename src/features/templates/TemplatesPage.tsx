/**
 * 转换模板列表页：页头 + 模板卡列表 + 空态。单卡解剖对齐任务卡家族，名称/备注单行 truncate ⇒ 卡高恒定（排序模型的地基）。
 * 拖拽排序用 pointer 事件手写实现（Tauri 在窗口级拦截 OS 文件拖入，HTML5 draggable 的 dragstart 不触发）；判据与积分器在 drag-sort.ts，本文件只管 DOM。
 * 硬约束：跟随层必须 portal 到 `body`；拖动全程 DOM 不搬移节点（拖动项只 `visibility:hidden`）；落位索引每帧现算且只认指针原始输入；出界/被打断回原序不写盘；`paint` 减的家位必须用 `useLayoutEffect` 里 DOM 真搬完的顺序。
 * 卡身不做 hover 样式（动作三枚常显、整卡不可点）；写盘只走 `use-template-table` 的 `commit`（与编辑页同口径）。
 * 图标分工：`LayoutTemplate` 只给侧栏导航；`FileSliders` 代表一张模板（卡片与空态共用）。
 */
import { AlertTriangle, Info, Plus } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { motion, useReducedMotion } from "motion/react";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useNavigation } from "@/lib/navigation";
import { uniqueTemplateName, type ConversionTemplate } from "@/lib/types";
import { Btn, NoteRow, PageHeader, Swap } from "@/components/ui";
import { ENTRY, entryStepMs, playEntry } from "@/lib/entry-curve";
import { DeleteTemplateModal } from "./DeleteTemplateModal";
import { EmptyTemplates, LoadingTemplates } from "./TemplatePlaceholders";
import { TemplateRow } from "./TemplateRow";
import { guardTemplateCap, useTemplateTable } from "./use-template-table";
import {
    MAX_DT,
    SORT,
    clampTop,
    homeOf,
    inRange,
    integrate,
    landEase,
    magnet,
    natIndex,
    slotOf,
    spliceOrder,
    settled,
    stepIndex,
    swayAngle,
    swayEnv,
    type Spring,
} from "./drag-sort";

/** 行距：单卡时步距量不到，用它兜底（除法才不会出 Infinity）——必须与列表那头的 `gap-2` 同源 */
const FALLBACK_GAP = 8;


/** 抬起那一刻量到的列几何：卡高与竖向步距（家位一律由「顺序 × 步距」算，不逐帧读 DOM） */
interface Column {
    height: number;
    pitch: number;
    gap: number;
}

/** 一行一份的手写弹簧：`y` 是容器坐标里的顶，`hold` 是错峰还没到起步的那一刻 */
interface RowMotion extends Spring {
    hold: number;
    el: HTMLElement | null;
}

/**
 * 一趟拖动会话。`active` 之前只是「按住了把手」，超过阈值才抬起；
 * `open` = 空槽开着（其余行的目标是让开一格），松手出界就关掉 ⇒ 行退回各家位；
 * `landing` = 已松手、抬起层正在走 `SORT.landMs` 那条加速补间；`commit` = 收敛完要不要写盘。
 */
interface DragSession {
    id: string;
    /** 指针在卡内的纵向落点 ⇒ 抬起层按它对齐，卡不会在拿起那一瞬跳到指针正中 */
    dy: number;
    height: number;
    left: number;
    width: number;
    startY: number;
    pointerY: number;
    /** 逐帧速度（抬起层不吃 X，这一路只服务那个来回摆：速度决定摆的幅度，不决定倒向哪一边） */
    pointerX: number;
    lastX: number;
    lastY: number;
    vx: number;
    vy: number;
    /** 来回摆的两个状态量：相位（弧度，按 `SORT.swayMs` 推进）与活跃度包络（0..1） */
    swayPhase: number;
    swayEnv: number;
    /** 这一帧的角度，直接写进 `rotate` */
    angle: number;
    active: boolean;
    homeIdx: number;
    /** 拖动项之外那些卡的顺序（整趟不变）与各自的位次，插入位就落在这条序列里 */
    others: string[];
    rank: Map<string, number>;
    idx: number;
    open: boolean;
    landing: boolean;
    commit: boolean;
    /** 落位补间的三个起点量：那一刻的位置/时间/角度，之后每帧按 `u` 插值 */
    landFrom: number;
    landT0: number;
    angleFrom: number;
    /** 最近一帧的「指针中心相对列表盒顶」：松手那一帧要拿它判落点在不在范围内 */
    rel: number;
    boxTop: number;
}

/** 抬起层：portal 到 body 上的那一张，位置由跟随弹簧写 */
interface Lift {
    id: string;
    left: number;
    width: number;
    height: number;
}

export function TemplatesPage() {
    const t = useT();
    const { navigate } = useNavigation();
    /** 系统「减少动态效果」：位移全部到位，不再逐帧积分（磁吸与滞回也就不需要演） */
    const reduced = useReducedMotion();
    const { templates, loaded, commit } = useTemplateTable();
    const [lift, setLift] = useState<Lift | null>(null);
    const [pendingDelete, setPendingDelete] = useState<ConversionTemplate | null>(null);

    const listRef = useRef<HTMLDivElement | null>(null);
    const cloneRef = useRef<HTMLDivElement | null>(null);
    const slotRef = useRef<HTMLDivElement | null>(null);
    const dragRef = useRef<DragSession | null>(null);
    /** 这一趟挂在 window 上的监听拆除口（挂的时候顺手记下，收尾与卸载都走它） */
    const offRef = useRef<(() => void) | null>(null);

    /**
     * 逻辑顺序（id 序列）：屏上「应该长成什么样」，落位收敛的那一帧就换成新的一份。
     * 之所以自己攥着而不读 `templates`：React 那一侧的顺序在这次提交之前不会变，
     * 而紧接着再来一趟拖动时，家位必须已经按新的排——读 render 闭包会拿到旧的那一份。
     */
    const orderRef = useRef<string[]>(templates.map((x) => x.id));
    /**
     * DOM 那份顺序：`paint` 减的家位用它（见文件头硬约束）。它**只**在 `useLayoutEffect` 里更新——
     * 那里 React 已经把节点搬完了，布局也量得到，所以「节点序」与「这份表」保证同一刻成立。
     * 落位收尾那一刻故意让它比 `orderRef` 旧一格：卡片靠 transform 停在新位置，等 React 重排完
     * 再归 0。提前改它就是整列闪一下的根因。
     */
    const domOrderRef = useRef<string[]>(orderRef.current);
    const colRef = useRef<Column>({ height: 1, pitch: 1, gap: FALLBACK_GAP });
    const springsRef = useRef(new Map<string, RowMotion>());
    const follower = useRef<Spring>({ y: 0, v: 0 });
    const slot = useRef<Spring>({ y: 0, v: 0 });
    const rafRef = useRef(0);
    const lastRef = useRef(0);
    /**
     * 长期挂在 window 上的那对闭包活过整趟拖动，所以它们读的表必须是**当前**那一份：
     * 每 render 抄一次进 ref（和 `orderRef` 同一口径：不认 render 闭包）。
     */
    const rowsRef = useRef(templates);
    rowsRef.current = templates;

    /** 拖动期间把光标与文字选中一起按住：只压 cursor 会漏下「一拖选蓝一片」 */
    const setGrabbing = (on: boolean) => {
        document.body.style.cursor = on ? "grabbing" : "";
        document.body.style.userSelect = on ? "none" : "";
    };

    /** 步距与卡高：读 `offsetTop`/`offsetHeight`（**布局坐标，不吃 transform**），所以行正在飞也量得准 */
    const measureColumn = (): Column => {
        const box = listRef.current;
        const kids = box ? (Array.from(box.children) as HTMLElement[]) : [];
        const height = kids[0]?.offsetHeight ?? 1;
        const raw = kids.length > 1 ? kids[1].offsetTop - kids[0].offsetTop : 0;
        const pitch = raw > 0 ? raw : height + FALLBACK_GAP;
        return { height, pitch, gap: Math.max(0, pitch - height) };
    };

    /** 把「弹簧顶 − 家位」写成 transform；家位减的是 **DOM 那份顺序**（第 8 条），不是逻辑顺序 */
    const paint = () => {
        const pitch = colRef.current.pitch;
        const order = domOrderRef.current;
        for (const id of order) {
            const s = springsRef.current.get(id);
            if (!s?.el) continue;
            const home = homeOf(order.indexOf(id), pitch);
            s.el.style.transform = Number.isFinite(s.y)
                ? `translate3d(0,${(s.y - home).toFixed(2)}px,0)`
                : "";
        }
        if (slotRef.current) {
            slotRef.current.style.transform = `translate3d(0,${slot.current.y.toFixed(2)}px,0)`;
        }
        const d = dragRef.current;
        if (cloneRef.current && d?.active) {
            cloneRef.current.style.transform = `translate3d(${d.left.toFixed(2)}px,${follower.current.y.toFixed(2)}px,0) rotate(${d.angle.toFixed(2)}deg)`;
        }
    };

    /** 起步错峰：离落点越远的行越晚开始追（整列同帧起步会读成「一坨一起挪」） */
    const stagger = (now: number, idx: number, d: DragSession) => {
        for (const id of d.others) {
            const s = springsRef.current.get(id);
            if (!s) continue;
            const rank = d.rank.get(id) ?? 0;
            s.hold = now + SORT.staggerMs * Math.min(SORT.staggerSteps, Math.abs(rank - idx));
        }
    };

    /**
     * 每帧一次：算索引 → 抬起层与空槽各追自己的目标 → 每行独立追赶。
     * `busy` 只在「还在拖」或「还有行没到位」时续帧，空闲下来整个循环停摆，不吃 CPU。
     */
    const frame = (now: number) => {
        rafRef.current = 0;
        const dt = lastRef.current ? Math.min(MAX_DT, (now - lastRef.current) / 1000) : 0;
        lastRef.current = now;
        const col = colRef.current;
        let d = dragRef.current;
        const soft = reduced === true;
        let busy = false;

        // 1) 抬起层：每帧一次读列表盒顶（页面被滚走也吃进来），索引只认原始输入
        if (d?.active) {
            busy = true;
            d.boxTop = listRef.current?.getBoundingClientRect().top ?? d.boxTop;
            // 甩出窗口的那几帧：先夹回边界再喂判据，于是位置与落点一起冻住
            //（只夹渲染会做出「卡停在边界、空槽还在往里挪」那种对不上）
            const rawTop = clampTop(
                d.pointerY - d.dy,
                d.height,
                window.innerHeight,
                SORT.edgeInset
            );
            if (!d.landing) {
                d.rel = rawTop + d.height / 2 - d.boxTop;
                const max = d.others.length;
                const next = stepIndex(
                    d.idx,
                    d.rel,
                    natIndex(d.rel, col.pitch, col.gap, max),
                    col.pitch,
                    SORT.band,
                    max
                );
                if (next !== d.idx) {
                    d.idx = next;
                    stagger(now, next, d);
                }
            }
            // 逐帧速度与活跃度包络：速度只决定摆的**幅度**，倒向哪一边由正弦自己决定（所以来回过零）
            const sdt = dt * 1000;
            if (dt > 0) {
                d.vx = (d.pointerX - d.lastX) / dt;
                d.vy = (d.pointerY - d.lastY) / dt;
                d.lastX = d.pointerX;
                d.lastY = d.pointerY;
            }
            const targetEnv = swayEnv(Math.hypot(d.vx, d.vy), SORT.swayFullPxS);
            // 一阶低通（时间常数 `swayEaseMs`）：起步不猛，停手后摆自己收干净，不留永动抖动
            const alpha = SORT.swayEaseMs > 0 ? Math.min(1, sdt / SORT.swayEaseMs) : 1;
            d.swayEnv += (targetEnv - d.swayEnv) * alpha;

            const slotTop = homeOf(d.open ? d.idx : d.homeIdx, col.pitch) + d.boxTop;
            if (soft) {
                follower.current.y = slotTop;
                follower.current.v = 0;
                d.angle = 0;
                if (d.landing) {
                    finishLanding();
                    d = null;
                    busy = false;
                }
            } else if (d.landing) {
                // 松手吸附：不走弹簧（那是振子，ζ<1 必过冲、ζ≥1 就变成「越拖越慢地贴过去」），
                // 走 u² ——起步慢、撞进格子那一瞬最快，且进度恒 ≤1，结构上没有越线那一步。
                // 时长固定不换行程：行程越长末速越大，「猛」的那一下就是这么来的。
                const u = landEase((now - d.landT0) / SORT.landMs);
                follower.current.y = d.landFrom + (slotTop - d.landFrom) * u;
                // 摆角跟着收平（相位冻住，只衰减幅值）：卡是摆平了吸进格子，不是带着斜度插进去
                d.angle = d.angleFrom * (1 - u);
                if (now - d.landT0 >= SORT.landMs) {
                    follower.current.y = slotTop;
                    d.angle = 0;
                    // 抬起层与下面那张卡站在同一个像素上 ⇒ 当场撤层。撤完不 return：
                    // 这一帧还要把各行改追自家位（漏掉它们就会把行程冻在半路）。
                    finishLanding();
                    d = null;
                    busy = false;
                }
            } else {
                integrate(
                    follower.current,
                    magnet(rawTop, slotTop, SORT.magnetRadius, SORT.magnetWeight).top,
                    SORT.followStiffness,
                    SORT.followDamping,
                    dt
                );
                d.swayPhase += (sdt / SORT.swayMs) * Math.PI * 2;
                d.angle = swayAngle(d.swayPhase, SORT.swayDeg, d.swayEnv);
            }
        }

        // 2) 每行一条独立弹簧：拖动中追「让开空槽后的那一格」，空闲追**逻辑**家位（`orderRef`）。
        //    落位提交后那几帧 DOM 还是旧序，家位差由 `paint` 减 `domOrderRef` 补回来（见文件头硬约束）
        const order = orderRef.current;
        for (const id of order) {
            const s = springsRef.current.get(id);
            if (!s) continue;
            const home = homeOf(order.indexOf(id), col.pitch);
            if (!Number.isFinite(s.y)) {
                s.y = home;
                continue;
            }
            if (d?.active && d.open) {
                // 离场那张：冻在自己的家位（它已经不可见，格子留着当容器高度的地基）
                if (id === d.id) {
                    s.y = home;
                    s.v = 0;
                    continue;
                }
            }
            const target =
                d?.active && d.open
                    ? slotOf(d.rank.get(id) ?? 0, d.idx, col.pitch)
                    : home;
            if (soft) {
                s.y = target;
                s.v = 0;
                continue;
            }
            if (now < s.hold) {
                busy = true;
                continue;
            }
            integrate(s, target, SORT.rowStiffness, SORT.rowDamping, dt);
            if (settled(s, target, SORT.rowSettlePx, SORT.rowSettleV)) {
                s.y = target;
                s.v = 0;
            } else busy = true;
        }

        // 3) 空槽自己也是一条弹簧：它跟着落点走，不是瞬时贴过去
        if (d?.active && d.open) {
            const target = homeOf(d.idx, col.pitch);
            if (soft) {
                slot.current.y = target;
                slot.current.v = 0;
            } else {
                integrate(slot.current, target, SORT.rowStiffness, SORT.rowDamping, dt);
                if (settled(slot.current, target, SORT.rowSettlePx, SORT.rowSettleV)) {
                    slot.current.y = target;
                    slot.current.v = 0;
                }
            }
        }

        paint();
        if (busy) rafRef.current = requestAnimationFrame(frame);
        else lastRef.current = 0;
    };

    /** 唤醒循环（空闲时才排帧，避免同一帧跑两遍） */
    const kick = () => {
        if (!rafRef.current) {
            lastRef.current = 0;
            rafRef.current = requestAnimationFrame(frame);
        }
    };

    /** 抬起那一瞬：量一次列几何、建显示序列（剔掉自己）、把抬起层与空槽摆在原位 */
    const activate = (d: DragSession) => {
        const col = measureColumn();
        colRef.current = col;
        // 序列读 `orderRef` 而不是数据表：上一趟刚提交的话，render 那一侧还没跟上（同一口径第 6 条）
        const ids = orderRef.current.length ? orderRef.current : rowsRef.current.map((x) => x.id);
        const others = ids.filter((x) => x !== d.id);
        d.homeIdx = Math.max(0, ids.indexOf(d.id));
        d.others = others;
        d.rank = new Map(others.map((x, i) => [x, i]));
        d.idx = d.homeIdx;
        d.open = true;
        d.active = true;
        d.boxTop = listRef.current?.getBoundingClientRect().top ?? 0;
        const rawTop = clampTop(d.pointerY - d.dy, d.height, window.innerHeight, SORT.edgeInset);
        d.rel = rawTop + d.height / 2 - d.boxTop;
        follower.current = { y: rawTop, v: 0 };
        // 摆从「平、静止」起步：上一趟留下的相位/包络/角度都不能带进这一趟
        d.lastX = d.pointerX;
        d.lastY = d.pointerY;
        d.vx = 0;
        d.vy = 0;
        d.swayPhase = 0;
        d.swayEnv = 0;
        d.angle = 0;
        slot.current = { y: homeOf(d.idx, col.pitch), v: 0 };
        const s = springsRef.current.get(d.id);
        if (s) {
            s.y = homeOf(d.homeIdx, col.pitch);
            s.v = 0;
        }
        stagger(performance.now(), d.idx, d);
        setGrabbing(true);
        setLift({ id: d.id, left: d.left, width: d.width, height: d.height });
    };

    /**
     * 松手：先把**这一帧**的落点算死（pointerup 常常跑在下一帧之前，只等 rAF 就少算一格），
     * 再判落点在不在范围内：在 ⇒ 空槽保持开着、收敛完写盘；不在 ⇒ 空槽关掉、行退回各家位、什么都不写。
     * `cancel` 那一档（触屏手势被抢、窗口失焦）不管落在哪都回原序——「取消不会误提交」走的是这条路。
     */
    const endGripDrag = (cancel = false) => {
        const d = dragRef.current;
        offRef.current?.();
        offRef.current = null;
        if (!d) return;
        if (!d.active) {
            dragRef.current = null;
            return;
        }
        const col = colRef.current;
        // 落点判定要用**当下**的盒顶：抬起层与行都还在弹簧上，上一帧之后页面也可能被滚走
        d.boxTop = listRef.current?.getBoundingClientRect().top ?? d.boxTop;
        const rawTop = clampTop(d.pointerY - d.dy, d.height, window.innerHeight, SORT.edgeInset);
        d.rel = rawTop + d.height / 2 - d.boxTop;
        const max = d.others.length;
        d.idx = stepIndex(
            d.idx,
            d.rel,
            natIndex(d.rel, col.pitch, col.gap, max),
            col.pitch,
            SORT.band,
            max
        );
        const keep = !cancel && inRange(d.rel, d.height, homeOf(max, col.pitch) + col.height);
        d.open = keep;
        d.commit = keep;
        d.landing = true;
        // 补间的三个起点量：位置从「抬起层此刻的视觉位置」起步（接着弹簧，不接弹簧目标），
        // 角度从当前摆角收平，时间用这一刻——下一帧的 `now` 只会比它晚，进度不会倒退
        d.landFrom = follower.current.y;
        d.landT0 = performance.now();
        d.angleFrom = d.angle;
        if (reduced === true) finishLanding();
        else kick();
    };

    /**
     * 落位收敛后的收尾：把逻辑顺序钉成新的一份，再撤抬起层。
     * 因为松手前每行都已追到「新顺序的家位」，而 `paint` 减的仍是 DOM 那份旧序 ⇒ 撤层那一帧下面那张卡
     * 就站在抬起层刚才那个像素上（transform 恒等于新旧格差），不跳、不补间、也不闪。
     * 这里**不许**动 `domOrderRef`：DOM 此刻还是旧节点序，减了新序就等于给所有行写 0 位移，
     * 整列当场退回拖之前的样子——那正是他报的「松手那一瞬全闪一下」。
     */
    const finishLanding = () => {
        const d = dragRef.current;
        if (!d) return;
        const col = colRef.current;
        const next = d.commit ? spliceOrder(d.others, d.id, d.idx) : orderRef.current;
        const list = rowsRef.current;
        const byId = new Map(list.map((x) => [x.id, x]));
        const rows = next
            .map((x) => byId.get(x))
            .filter((x): x is ConversionTemplate => x !== undefined);
        // 认不全（这一张刚被别处删掉）就当没动：一拖不该把某张卡变没，顺序也就退回 DOM 那份
        const apply = d.commit && rows.length === list.length;
        orderRef.current = apply ? next : domOrderRef.current;
        const s = springsRef.current.get(d.id);
        if (s) {
            s.y = homeOf(Math.max(0, orderRef.current.indexOf(d.id)), col.pitch);
            s.v = 0;
            s.hold = 0;
        }
        for (const id of d.others) {
            const other = springsRef.current.get(id);
            if (other) other.hold = 0;
        }
        dragRef.current = null;
        // 落位不总在 rAF 里（减少动态效果、上一趟没落完就再按把手）：这一帧没人 paint，
        // 抬起层一撤就把卡冻在半路了，所以收尾自己补一次
        paint();
        setLift(null);
        setGrabbing(false);
        if (!apply) return;
        const was = api.peekTemplates() ?? [];
        const changed = was.length !== rows.length || rows.some((x, i) => x.id !== was[i]?.id);
        if (changed) void commit(rows);
    };

    const onGripMove = (e: PointerEvent) => {
        const d = dragRef.current;
        if (!d) return;
        d.pointerY = e.clientY;
        // 横向只进这一个量：抬起层不吃 X，摆动吃的是「帧与帧之间它走了多远」
        d.pointerX = e.clientX;
        if (!d.active) {
            if (Math.abs(e.clientY - d.startY) < SORT.liftAfterPx) return;
            activate(d);
        }
        kick();
    };

    /**
     * 抬起那一按：先建会话，再把 move/up **挂到 `window`**——不挂把手那一行。
     * 上一趟还在落位（手指连点两下）就先把它的落点钉死再开新的一趟，别把两层抬起层留在屏上。
     */
    const onGripDown = (e: React.PointerEvent<HTMLElement>, id: string) => {
        if (e.button !== 0) return;
        // 上一趟还在落位（多指、连点两下）：先把监听撤干净、把它的落点钉死，再开新的一趟。
        // 顺序不能反——先开新会话就会把 `offRef` 覆盖掉，那一双旧闭包永远挂在 window 上。
        offRef.current?.();
        offRef.current = null;
        // 上一趟只按到一半没抬起（没建会话成功）就直接丢掉，别让它去走一遍落位
        if (dragRef.current?.active) finishLanding();
        else dragRef.current = null;
        // 卡片还在演换页升起就当场掐掉：下面量的是 rect，那一趟位移会让抬起层的起点整个偏一截
        entryUndos.current.forEach((stop) => stop());
        entryUndos.current = [];
        const card = (e.currentTarget.closest("[data-card-id]") as HTMLElement | null)
            ?.getBoundingClientRect();
        if (!card) return;
        // 压掉这一按的默认动作：不然从把手起步的那一拖会顺手把卡上的文字选中成一片蓝
        e.preventDefault();
        // 捕获仍然要：它买的是「指针滑出窗口也照样收得到 up」。但掉了捕获不算收尾信号，我们只认 up。
        e.currentTarget.setPointerCapture(e.pointerId);
        dragRef.current = {
            id,
            dy: e.clientY - card.top,
            height: card.height,
            left: card.left,
            width: card.width,
            startY: e.clientY,
            pointerY: e.clientY,
            pointerX: e.clientX,
            lastX: e.clientX,
            lastY: e.clientY,
            vx: 0,
            vy: 0,
            swayPhase: 0,
            swayEnv: 0,
            angle: 0,
            active: false,
            homeIdx: 0,
            others: [],
            rank: new Map(),
            idx: 0,
            open: false,
            landing: false,
            commit: false,
            landFrom: 0,
            landT0: 0,
            angleFrom: 0,
            rel: 0,
            boxTop: 0,
        };
        // `up` 必须包一层：直接把 `endGripDrag` 当监听器，PointerEvent 就是那个 `cancel` 参数（真值），
        // 每一趟都会被当成「被打断」而回原序——这正是「拖过去了却没排上」的新版写法，别这么接。
        const up = () => endGripDrag(false);
        const cancel = (ev: PointerEvent) => {
            // 触屏那档 cancel 之后不会再有 up，所以它必须收尾（且算被打断、不提交）；
            // 鼠标那档多半是系统/原生拖拽抢了手势，up 还会来，这里别动
            if (ev.pointerType !== "mouse") endGripDrag(true);
        };
        // 真·既没 up 也没 cancel 的那一档（手势被整个吞掉）：失焦时收个尾，同样不算提交
        const loseFocus = () => endGripDrag(true);
        window.addEventListener("pointermove", onGripMove);
        window.addEventListener("pointerup", up);
        window.addEventListener("pointercancel", cancel);
        window.addEventListener("blur", loseFocus);
        offRef.current = () => {
            window.removeEventListener("pointermove", onGripMove);
            window.removeEventListener("pointerup", up);
            window.removeEventListener("pointercancel", cancel);
            window.removeEventListener("blur", loseFocus);
        };
    };

    /**
     * 行内那层 transform 的挂点：登记元素，卸载时只撤引用（弹簧状态留着，换位才有「从哪儿来」）。
     */
    const registerRow = (id: string, el: HTMLElement | null) => {
        const s = springsRef.current.get(id);
        if (!el) {
            if (s) s.el = null;
            return;
        }
        if (s) s.el = el;
        else springsRef.current.set(id, { y: Number.NaN, v: 0, hold: 0, el });
    };

    /**
     * 数据一变（新增/删除/复制/编辑保存/落位提交）就重量列几何、把家位差补进 transform，
     * 剩下的行程交给同一条积分器——所以增删换位不需要 motion 的 `layout`。
     * 这一层还是 `domOrderRef` 唯一的写口：回调跑在 React 把节点搬完之后、绘制之前，
     * 所以「DOM 序 = 这份表」在这里恒成立，落位那一帧的 transform 也就正好归 0（第 8 条）。
     */
    useLayoutEffect(() => {
        const ids = templates.map((x) => x.id);
        colRef.current = measureColumn();
        domOrderRef.current = ids;
        if (!dragRef.current) orderRef.current = ids;
        const alive = new Set(ids);
        for (const id of orderRef.current) alive.add(id);
        for (const id of Array.from(springsRef.current.keys())) {
            if (!alive.has(id)) springsRef.current.delete(id);
        }
        paint();
        kick();
    }, [templates, loaded]);

    /**
     * 「骨架/空态 → 列表」那一拍：在场每张卡从下方升起、回弹一档（比任务页轻的 `softAmpPx`），逐卡错峰。
     * 只认 `listShown` 这一个开关，不按 `templates` 走 ⇒ 新增/删除/落位不演（那一路有积分器的补位弹簧，两层一起挪会读成抖）。
     * `useLayoutEffect` 是必需的：effect 晚了就是「先画出卡片、再把它抹到起点」，看着像闪一下。
     * 撤销函数留一份在 ref：起拖前要当场掐掉（`onGripDown` 量的是 rect，吃不得这层的位移），这一页卸掉时也掐掉。
     */
    const listShown = loaded && templates.length > 0;
    const entryUndos = useRef<(() => void)[]>([]);
    useLayoutEffect(() => {
        const nodes = listShown
            ? Array.from(listRef.current?.querySelectorAll<HTMLElement>("[data-entry]") ?? [])
            : [];
        if (!nodes.length) return;
        const step = reduced === true ? 0 : entryStepMs(nodes.length);
        entryUndos.current = nodes.map((el, i) => playEntry(el, i * step, ENTRY.softAmpPx));
        return () => {
            entryUndos.current.forEach((stop) => stop());
            entryUndos.current = [];
        };
    }, [listShown, reduced]);

    // 拖到一半被别处换走（这一页当场卸掉）⇒ pointerup 不会再来：撤监听、停循环，那根 grabbing 光标不能跟人跑到别的页
    useEffect(() => {
        return () => {
            offRef.current?.();
            offRef.current = null;
            if (rafRef.current) cancelAnimationFrame(rafRef.current);
            rafRef.current = 0;
            document.body.style.cursor = "";
            document.body.style.userSelect = "";
            springsRef.current.clear();
        };
    }, []);

    /** 复制 = 在原位的下一格插一份「X 副本」：位置跟着源卡走，人才找得到它。也是「建一份新的」，吃同一道闸门 */
    const duplicate = (tpl: ConversionTemplate) => {
        if (!guardTemplateCap(templates.length)) return;
        const at = templates.findIndex((x) => x.id === tpl.id);
        const want = `${tpl.name}${t("templates.copy-suffix", " 副本")}`;
        void commit([
            ...templates.slice(0, at + 1),
            {
                ...tpl,
                id: api.newTemplateId(),
                // 让开同名位：复制不该造出一枚编辑页会拦下的名字
                name: uniqueTemplateName(want, templates.map((x) => x.name)),
                updatedAt: Date.now(),
            },
            ...templates.slice(at + 1),
        ]);
    };

    const dragId = lift?.id ?? null;
    const liftTpl = lift ? templates.find((x) => x.id === lift.id) : undefined;

    return (
        <div className="flex flex-col gap-3 pb-6">
            {/* 吸顶：右上那颗「新建模板」是本页唯一的建入口，滚下去就够不着了。
                留白这圈由吸顶盒自己出（pt-6），pb-3 + -mb-3 抵掉根 div 的 gap-3 ⇒ 静止观感与
                不吸顶时逐像素相同。z-20 是必需的：行的位移写在 transform 上，带 transform 就是
                层叠上下文，按文档顺序会盖在页头上面。材质见 App.css 的 `.page-head-veil`。 */}
            <div className="page-head-veil sticky top-0 z-20 -mb-3 pt-6 pb-3">
                <PageHeader
                    title={t("shell.templates", "转换模板")}
                    sub={t("templates.head-sub", "存下常用的转换配置 · 转换页一键套用")}
                    right={
                        loaded && templates.length > 0 ? (
                            <Btn
                                variant="primary"
                                size="sm"
                                icon={Plus}
                                className="font-semibold"
                                onClick={() => {
                                    if (guardTemplateCap(templates.length)) navigate("template");
                                }}
                            >
                                {t("templates.new-template", "新建模板")}
                            </Btn>
                        ) : undefined
                    }
                />
            </div>

            {/* 骨架 → 空态/列表 是同位换批（首帧读数落地那一刻整块换掉），走全站共用的一对节拍 */}
            <Swap swapKey={!loaded ? "loading" : templates.length === 0 ? "empty" : "list"} className="gap-3">
                {!loaded ? (
                    <LoadingTemplates />
                ) : templates.length === 0 ? (
                    <EmptyTemplates />
                ) : (
                    <>
                        <div className="-mt-1 px-0.5">
                            <NoteRow icon={Info}>
                                {t(
                                    "templates.drag-hint",
                                    "拖动排序 · 同步至转换配置页的排序"
                                )}
                            </NoteRow>
                        </div>

                        {/* `relative` 是两件事的前提：空槽那层的定位基准，以及行 `offsetTop` 的坐标原点 */}
                        <div ref={listRef} className="relative flex flex-col gap-2">
                            {templates.map((tpl) => (
                                <motion.div
                                    key={tpl.id}
                                    data-card-id={tpl.id}
                                    // 位移归积分器管（内层那层 transform），motion 只管这层的淡入
                                    initial={{ opacity: 0 }}
                                    animate={{ opacity: 1 }}
                                    transition={{ duration: 0.18, ease: "easeOut" }}
                                >
                                    {/* 换页升起那一层：曲线只写这层的 transform/opacity，与内层积分器各写各的才不抢属性 */}
                                    <div data-entry>
                                        {/* 行自己的一层：拖动与换位都只改这层的 transform，节点整趟不搬 */}
                                        <div ref={(el) => registerRow(tpl.id, el)}>
                                            <TemplateRow
                                                template={tpl}
                                                lifted={dragId === tpl.id}
                                                onOpen={() => navigate("template", { templateId: tpl.id })}
                                                onCopy={() => duplicate(tpl)}
                                                onDelete={() => setPendingDelete(tpl)}
                                                onGripDown={(e) => onGripDown(e, tpl.id)}
                                            />
                                        </div>
                                    </div>
                                </motion.div>
                            ))}

                            {/* 空槽：拖动项脱离流之后，屏上那一格交给它（虚线 + 淡底，落点在哪它就在哪）。
                                挂载那一刻就要把 transform 写上：React 提交先于绘制，漏掉这一笔它会先在列表顶上闪一帧 */}
                            {lift && (
                                <div
                                    ref={(el) => {
                                        slotRef.current = el;
                                        if (el) {
                                            el.style.transform = `translate3d(0,${slot.current.y.toFixed(2)}px,0)`;
                                        }
                                    }}
                                    aria-hidden="true"
                                    className="pointer-events-none absolute left-0 top-0 right-0 rounded-[12px] border-[1.5px] border-dashed border-accent bg-accent-dim/50"
                                    style={{ height: lift.height }}
                                />
                            )}
                        </div>

                        <div className="px-0.5">
                            <NoteRow icon={AlertTriangle}>
                                {t(
                                    "templates.scope-hint",
                                    "MC 版本、加载器版本、包内保留内容不进模板"
                                )}
                            </NoteRow>
                        </div>
                    </>
                )}
            </Swap>

            {/*
              跟手那层：同一张卡的重绘件（不是 DOM 克隆——pointer 路径下我们手里就有数据）。
              宽度钉死在按下那一刻量到的值，所以它在流外也不换行、不跟着右栏伸缩；
              `pointer-events-none` 让它不吃命中。位置由跟随弹簧逐帧写 transform（含磁吸的那一点回弹）。
              **必须 portal 到 body**：见文件头硬约束。
            */}
            {lift &&
                liftTpl &&
                createPortal(
                    <div className="pointer-events-none fixed left-0 top-0 z-[999]" aria-hidden="true">
                        {/* 同上：这一层挂上就要落在指针下面，不然抬起那一瞬先在屏幕左上角闪一张卡 */}
                        <div
                            ref={(el) => {
                                cloneRef.current = el;
                                if (el) {
                                    // 与 `paint` 逐字同一串：挂上那一帧就得站在抬起层该在的像素与角度上
                                    el.style.transform = `translate3d(${(dragRef.current?.left ?? 0).toFixed(2)}px,${follower.current.y.toFixed(2)}px,0) rotate(${(dragRef.current?.angle ?? 0).toFixed(2)}deg)`;
                                }
                            }}
                            style={{ width: lift.width }}
                        >
                            <TemplateRow template={liftTpl} overlay />
                        </div>
                    </div>,
                    document.body
                )}

            <DeleteTemplateModal
                template={pendingDelete}
                onClose={() => setPendingDelete(null)}
                onConfirm={(tpl) => {
                    setPendingDelete(null);
                    void commit(templates.filter((x) => x.id !== tpl.id));
                }}
            />
        </div>
    );
}


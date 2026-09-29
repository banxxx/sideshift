/**
 * 转换模板列表页（设计稿 `.scratch/templates-page-proto.html` 屏 ①（有模板）/ ④（零模板））
 *
 * 结构：页头（标题 + 副标 + 右侧「新建模板」）→ 说明行 → 卡列表（gap8，与 `FALLBACK_GAP` 同源）→ 收口说明行。
 * 单卡对齐任务卡家族（TasksPage.tsx:548-575 的解剖）：p-5、36×36 r10 图标盒装 17px、
 * 主名等宽 13/20 600、副行等宽 11/16 400 text-3、列间 gap-12。名称与备注都单行 truncate，
 * 所以卡的**高度恒定**——这一条是排序模型的地基，见下面第 6 条。
 *
 * 三枚动作常显（不玩「悬停才出现」），而且**动作只有这三枚**：整卡不再可点进编辑。
 * 卡是这一族数据的容器、不是按钮，卡片底色跟着 hover 压深会把它读成一个「点这里」的出口
 * （同「不可点必须灰化」那条口径）⇒ 卡身不换底色不换描边，只有把手与三枚按钮各自有悬停档。
 *
 * 排序用 **pointer 事件自己实现**，不用 HTML5 拖拽：本应用靠 Tauri 的 `onDragDropEvent` 收 OS 文件
 * 拖入（首页选包那条链，见 home-state.ts 的 useTauriFileDrop），而这份拦截默认开着 ⇒ WebView 里
 * `draggable` 的 dragstart 根本不触发。换 pointer 的好处是不必为了排序去翻窗口级配置、动到首页拖包。
 *
 * 拖动那一套是「脱离流 + 空槽 + 磁吸」模型（样片 `.scratch/templates-drag-magnet-proto.html`），
 * 判据与积分器都在 drag-sort.ts，这个文件只管 DOM。八条是地基，别再写回去。
 * 手感两档已定案（都只有一个旋钮，别再改回旧的写法）：**抬起层来回摆**（一条正弦 `swayAngle`，速度只喂
 * 幅度、不决定倒向 ⇒ 两个方向轮着来，捏着不动包络自己收到 0；旧的「静止带一档固定倾角」被他打回过，
 * 说那是固定倒向一边、不是动起来），**松手吸附不带弹簧**（走 `SORT.landMs` 的 u² 加速补间：起步慢、
 * 撞进格子最快，恒 ≤1 所以不可能反弹）。
 *  1. **跟手那层必须 portal 到 `body`**：页面挂在 `motion.main` 里，那块既有 `y` 动画（transform ⇒ 它成了
 *     fixed 后代的包含块，视口坐标当场算错）又带 `overflow-auto`（fixed 后代照样被裁）。同 App.tsx:110
 *     垃圾桶为什么留在 Shell 层——别赌「fixed 后代能不能穿过滚动容器」。
 *  2. **整趟拖动 DOM 一个节点都不搬**：拖动项只是 `visibility:hidden`（格子还占着，容器高度恒定 ⇒ 不跳版），
 *     其余行的位移一律写在自己的 `transform` 上。以前每换一次格就让 React 在那排兄弟里摘一次、插一次，
 *     而 `setPointerCapture` 与挂在行上的监听认的都是**节点**——节点一搬捕获当场掉，「还按着」被读成
 *     「已松手」，那就是他报的「往下拖到某个位置自己松了」。这一版从结构上没有了那一路。
 *  3. **落位索引每帧现算，而且只认原始输入**（指针算出来的顶，且**先夹进窗口**再喂判据：甩出上下沿那几帧
 *     位置与落点一起冻在边界，不会出现「卡不动、空槽还在挪」），不认磁吸混合后的位置。混合位置再喂回索引，
 *     吸附就会自己拖着索引连锁跳格。每帧只读一次列表盒的 `rect.top`（为了吃进滚动），行的边界不吃 rect——
 *     行正在弹簧上飞，读 rect 就读到半路那条线。
 *  4. **格线按「第几格」算**：`第几格 = (指针中心 + 半条行距) / 步距`，恒等式「克隆中心 ∈ 自己那一格」才立得住，
 *     上下两个方向共用同一条线，没有旧中线的半格偏置。再加进出各半带的滞回（`SORT.band`）压掉交界抖动。
 *  5. **会话期间的事件源挂 `window`，不挂把手那一行**；能结束会话的信号只有 `pointerup`——
 *     `lostpointercapture` 不算，`pointercancel`（系统抢手势/原生拖拽起步）只当噪音（触屏那档 cancel 之后
 *     没有 up，所以它必须收尾），真·既没 up 也没 cancel 的那一档用 `window` 的 `blur` 兜住。
 *     再把把手的 `draggable` 钉成 `false`，连图标起步原生拖拽这一路一起堵掉。
 *  6. **位置 = 弹簧顶 − 网格家位**，家位由「自己维护的显示顺序 × 步距」算死，不读 DOM。于是松手那一帧
 *     落点正好等于新家的位置：抬起层撤掉时下面那张卡就在同一像素上，不需要任何补间、也不会重排一次。
 *     这也是为什么不用 motion 的 `layout`：那条弹簧只在投影节点创建时读一次目标，演不出「每帧换一个落点」；
 *     而增删换位复用同一条积分器（数据一变，家位一变，行自己追过去），全站这一族只剩一个运动件。
 *  7. **提交只有一条路、且只在落位收敛之后**：出界（指针中心掉出列表）与被打断一律回原序、不写盘；
 *     只有顺序真的变了才 `commit`（半途写会把「拖完又拖回来」的中间态留在磁盘上）。
 *  8. **`paint` 减的家位必须是 DOM 真搬完的那份顺序**（`domOrderRef`，只在 `useLayoutEffect` 里更新），
 *     落点写的是逻辑顺序（`orderRef`）。松手那一帧两者故意差一格：transform = 新格顶 − 旧格顶 ≠ 0，
 *     卡就还停在新位置；等 React 把节点搬完、布局回调把 `domOrderRef` 换过来，transform 当场归 0——同一像素。
 *     早一步在收尾里写 `domOrderRef` 就会让所有卡在「旧节点序」上拿到 0 位移 ⇒ 整列闪回拖之前的样子
 *     （React 19 里 rAF 触发的更新经 MessageChannel 宏任务提交，常常排在**这一帧的绘制之后**，所以那一帧真看得见）。
 *
 * 写盘只有一条路：整张表交回后端，乐观更新与失败回滚都在 `use-template-table`（编辑页共用同一份口径）。
 */
import { AlertTriangle, Copy, GripVertical, Info, LayoutTemplate, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { motion, useReducedMotion } from "motion/react";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useNavigation } from "@/lib/navigation";
import { templateValueCount, uniqueTemplateName, type ConversionTemplate } from "@/lib/types";
import { Btn, IconBtn, NoteRow, PageHeader, Swap } from "@/components/ui";
import { DeleteTemplateModal } from "./DeleteTemplateModal";
import { useTemplateTable } from "./use-template-table";
import { cn } from "@/lib/utils";
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

/**
 * 抬起层那三枚按钮禁的是「键盘还能 Tab 进去」（那层已 `aria-hidden`，可聚焦的 child 站在隐形层上），
 * 不是「它们不能按」。所以把 `IconBtn` 自带的 `disabled:opacity-40` 压回原样——
 * 跟手那张必须和流内那张一模一样，他否决过任何让拖影看起来不同的做法。
 */
const LIFT_INERT = "disabled:opacity-100";

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
     * DOM 那份顺序：`paint` 减的家位用它（文件头第 8 条）。它**只**在 `useLayoutEffect` 里更新——
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
        //    落位提交后那几帧 DOM 还是旧序，家位差由 `paint` 减 `domOrderRef` 补回来（文件头第 8 条）
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
     * 抬起那一按：先建会话，再把 move/up **挂到 `window`**——不挂把手那一行（文件头第 5 条）。
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

    /** 复制 = 在原位的下一格插一份「X 副本」：位置跟着源卡走，人才找得到它 */
    const duplicate = (tpl: ConversionTemplate) => {
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
        <div className="flex flex-col gap-3 py-6">
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
                            onClick={() => navigate("template")}
                        >
                            {t("templates.new-template", "新建模板")}
                        </Btn>
                    ) : undefined
                }
            />

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
                                    "拖动左侧把手排序 · 这里的顺序就是转换页那颗下拉的顺序 · 点铅笔进编辑"
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
                                    "只收「换个包还要一样」的参数 · MC 版本、加载器版本、包内保留内容不进模板"
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
              **必须 portal 到 body**：见文件头第 1 条。
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

/* ---------------- 单卡 ---------------- */

/**
 * 一张模板卡。两处复用同一份 markup：列表里那张（可拖、有三枚动作）与抬起层那张（`overlay`，
 * 只多一层投影、不吃命中、动作按钮不再挂着），所以「跟手的是这张卡本体」这条读感才立得住。
 */
function TemplateRow({
    template,
    lifted,
    overlay,
    onOpen,
    onCopy,
    onDelete,
    onGripDown,
}: {
    template: ConversionTemplate;
    /** 这张已经脱离显示序列、由抬起层代管：格子留着（容器高度恒定才不跳版），卡本身收起来 */
    lifted?: boolean;
    /** 抬起层：投影 + 不吃命中，动作件不再要交互 */
    overlay?: boolean;
    onOpen?: () => void;
    onCopy?: () => void;
    onDelete?: () => void;
    onGripDown?: (e: React.PointerEvent<HTMLElement>) => void;
}) {
    const t = useT();
    const count = templateValueCount(template.values);
    return (
        <section
            className={cn(
                // 卡身没有 hover 档：整卡不可点，hover 压深会把它读成一个假出口
                "group flex items-center gap-3 rounded-[12px] border bg-surface p-5",
                overlay ? "border-accent shadow-lg" : "border-stroke",
                lifted && "invisible"
            )}
        >
            {/* 拖动把手：命中区**吃满整卡高度**（`self-stretch`，实测 79px 而不是原来那 32px 小格）——
                它是唯一的拖动入口，命中区太小就直接读成「拖不动」。`touch-none` 让指针不被系统滚动手势吃掉，
                `draggable={false}` 挡掉「从图标起步拖出原生拖拽」那一路（它一发 `pointercancel` 手势就归它了）。
                这一按之后**不再挂任何 move/up**：整趟监听都挂在 `window` 上，见 `onGripDown`。
                整卡 draggable 那条已经删了——Tauri 的 OS 拖入拦截开着，dragstart 本来就不会来。 */}
            <span
                onPointerDown={onGripDown}
                draggable={false}
                className={cn(
                    "flex w-5 shrink-0 self-stretch touch-none items-center justify-center rounded-lg text-text-3",
                    "transition-colors group-hover:text-text-2",
                    !overlay && "cursor-grab active:cursor-grabbing"
                )}
            >
                <GripVertical className="size-3.5" />
            </span>
            <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-accent-dim text-accent">
                <LayoutTemplate className="size-[17px]" />
            </span>
            <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <span className="truncate font-mono text-[13px] leading-[20px] font-semibold text-text-1">
                    {template.name}
                </span>
                <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {template.note || t("templates.no-note", "未填备注")}
                </span>
            </span>
            {/* 两轨分界：左边「这张卡叫什么」，右边「它收了几档 + 能做什么」 */}
            <span className="h-7 w-px shrink-0 bg-stroke-soft" />
            <span className="min-w-[34px] shrink-0 text-right font-mono text-[11px] leading-[16px] font-normal text-text-3">
                {t("templates.n-items", "{{count}} 项", { count })}
            </span>
            <span className="flex shrink-0 items-center gap-2">
                <IconBtn
                    icon={Pencil}
                    title={t("templates.edit", "编辑")}
                    className={LIFT_INERT}
                    disabled={overlay}
                    onClick={onOpen}
                />
                <IconBtn
                    icon={Copy}
                    title={t("templates.duplicate", "复制")}
                    className={LIFT_INERT}
                    disabled={overlay}
                    onClick={onCopy}
                />
                <IconBtn
                    icon={Trash2}
                    title={t("templates.delete", "删除")}
                    className={cn("hover:bg-redstone-dim hover:text-redstone", LIFT_INERT)}
                    disabled={overlay}
                    onClick={onDelete}
                />
            </span>
        </section>
    );
}

/* ---------------- 首轮读数占位 / 零模板空态（屏 ④） ---------------- */

/** 占位照真实卡排（同一套 p-5 / 36 图标盒 / 双行文字高度 / `gap-2` 行距）：读数到手时高度几乎不动 */
function LoadingTemplates() {
    return (
        <div className="flex flex-col gap-2">
            {Array.from({ length: 3 }, (_, i) => (
                <div
                    key={i}
                    className="flex items-center gap-3 rounded-[12px] border border-stroke bg-surface p-5"
                >
                    <span className="size-9 shrink-0 animate-pulse rounded-[10px] bg-surface-2" />
                    <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
                        <span className="h-[13px] w-36 animate-pulse rounded bg-stroke" />
                        <span className="h-[10px] w-52 animate-pulse rounded bg-stroke-soft" />
                    </span>
                    <span className="h-[11px] w-9 shrink-0 animate-pulse rounded bg-stroke-soft" />
                    <span className="h-8 w-24 shrink-0 animate-pulse rounded-lg bg-stroke" />
                </div>
            ))}
        </div>
    );
}

/** 空态解剖照搬 EmptyTasks（TasksPage.tsx:389-416），一处不改：图标盒 56 r20、标题等宽 16/24、CTA 同款覆盖 */
function EmptyTemplates() {
    const t = useT();
    const { navigate } = useNavigation();
    return (
        <div className="flex h-[clamp(400px,70vh,640px)] flex-col items-center justify-center gap-4 rounded-[12px] bg-bg-app px-5 py-10">
            <div className="flex flex-col items-center gap-4">
                <span className="flex size-14 items-center justify-center rounded-2xl bg-surface-2">
                    <LayoutTemplate className="size-6 text-text-3" />
                </span>
                <div className="flex flex-col items-center gap-1">
                    <span className="font-mono text-[16px] leading-[24px] font-semibold text-text-1">
                        {t("templates.none-yet", "还没有模板")}
                    </span>
                    <span className="font-mono text-[12px] leading-[18px] font-normal text-text-3">
                        {t("templates.empty-sub", "在转换页把参数调好，存一份下次直接套用")}
                    </span>
                </div>
            </div>
            <Btn
                variant="primary"
                icon={Plus}
                className="border border-stroke text-[12px] font-medium"
                onClick={() => navigate("template")}
            >
                {t("templates.new-template", "新建模板")}
            </Btn>
        </div>
    );
}

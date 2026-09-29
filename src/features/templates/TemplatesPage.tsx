/**
 * 转换模板列表页（设计稿 `.scratch/templates-page-proto.html` 屏 ①（有模板）/ ④（零模板））
 *
 * 结构：页头（标题 + 副标 + 右侧「新建模板」）→ 说明行 → 卡列表（gap16）→ 收口说明行。
 * 单卡对齐任务卡家族（TasksPage.tsx:548-575 的解剖）：p-5、36×36 r10 图标盒装 17px、
 * 主名等宽 13/20 600、副行等宽 11/16 400 text-3、列间 gap-12。名称与备注都单行 truncate，
 * 所以卡的**高度恒定**——排序换位的补位弹簧才不会被不等高卡片演成跳动。
 *
 * 三枚动作常显（不玩「悬停才出现」），而且**动作只有这三枚**：整卡不再可点进编辑。
 * 卡是这一族数据的容器、不是按钮，卡片底色跟着 hover 压深会把它读成一个「点这里」的出口
 * （同「不可点必须灰化」那条口径）⇒ 卡身不换底色不换描边，只有把手与三枚按钮各自有悬停档。
 *
 * 排序用 **pointer 事件自己实现**，不用 HTML5 拖拽：本应用靠 Tauri 的 `onDragDropEvent` 收 OS 文件
 * 拖入（首页选包那条链，见 home-state.ts 的 useTauriFileDrop），而这份拦截默认开着 ⇒ WebView 里
 * `draggable` 的 dragstart 根本不触发（稿外说明早就写过这条天花板：要 100% 可控就得换 pointer）。
 * 换 pointer 的好处是不必为了排序去翻窗口级配置、动到首页拖包。
 * 手感照稿内那套：留在流里的那张只点亮描边，跟手的是同一张卡的重绘件 + 一层投影（不半透明）。
 *
 * 这一套里五条是修出来的病根，别再写回去：
 *  1. **跟手那层必须 portal 到 `body`**：页面挂在 `motion.main` 里，那块既有 `y` 动画（transform ⇒ 它成了
 *     fixed 后代的包含块，视口坐标当场算错）又带 `overflow-auto`（fixed 后代照样被裁）。同 App.tsx:110
 *     垃圾桶为什么留在 Shell 层——别赌「fixed 后代能不能穿过滚动容器」。
 *  2. **落位几何只在抬起那一刻量一次，且读 `offsetTop`（布局坐标）而不是 rect（视觉坐标）**。
 *     逐帧现读各卡 rect 有两条死：一是每根 pointermove 一次强制布局，二是补位弹簧要跑 ≈400ms，
 *     rect 读到的是「正在飞的那条线」——连着拖两下、第二趟在第一趟没落定时抬起，边界当场算歪，
 *     同一次越过还可能被算成两次换位 ⇒ 来回抖，看着就像卡住不动、或「拖过去了却没排上」。
 *  3. **换位每帧最多算一次**（rAF 合流），松手时再当场补算一次定落点。高刷新率鼠标一根线能报上千次
 *     pointermove，逐次 setState 就是逐次重挂投影；而落点若只等 rAF，pointerup 常常跑在下一帧之前，
 *     那一格就永远没算进去——「拖了却排不上」是那一条。写盘同样只认这份当场算死的顺序，
 *     不认 render 闭包里的 `templates`（它可能还差最后一次换位）。
 *  4. **判据线必须是「第几格」而不是「当初那条中线」**：留在流里的那张卡占着一格，所以整列表永远是
 *     N 格；一换位，被跨过的那行就挪走一整格，而它**抬起时量到的中线还钉在原地**。于是「保持这一格」
 *     的区间只剩自己那一格的下半——中心一抬过格心，顺序当场弹回，可屏幕上那张还好好压在下一格上。
 *     那就是「拖过去了却没排上」（松手时正好停在那半格里）与「抖一下又回去」的同一条根。
 *     换成按格算：`第几格 = (指针中心 + 半格间隙) / 步距`，恒等式「克隆中心 ∈ 自己那一格」才立得住，
 *     上下两个方向共用同一条格线，不再有半格的偏置。步距从相邻两行的 `offsetTop` 差里量。
 *  5. **会话期间的事件源挂 `window`，不挂把手那一行；「掉了捕获」「手势被抢」都不算收尾信号**。
 *     每换一次位，React 就在那一排兄弟里挪一个节点（摘掉再插回）——而 `setPointerCapture` 与挂在行上的
 *     `pointermove/up/cancel/lostpointercapture` 认的都是**节点**。节点一搬，捕获可能当场掉，于是
 *     「还按着」被读成「已松手」，抬起层就地弹回：那就是「往下拖到某个位置自己松了」。
 *     哪一次搬动会掉捕获（向下必掉、向上看着没事）我实测不到，判据只能给到这里：**能结束会话的信号只剩
 *     `pointerup`**——`lostpointercapture` 撤掉，`pointercancel`（系统抢手势/原生拖拽起步都会发它）也只当噪音，
 *     因为 up 一定还会来；真·没 up 的那一档用 `window` 的 `blur` 兜住。再把把手的 `draggable` 钉成 `false`，
 *     连图标起步原生拖拽这一路一起堵掉。
 *
 * 写盘只有一条路：整张表交回后端，乐观更新与失败回滚都在 `use-template-table`（编辑页共用同一份口径）。
 */
import { AlertTriangle, Copy, GripVertical, Info, LayoutTemplate, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { motion, useMotionValue } from "motion/react";
import * as api from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useNavigation } from "@/lib/navigation";
import { REFLOW } from "@/lib/springs";
import { templateValueCount, uniqueTemplateName, type ConversionTemplate } from "@/lib/types";
import { Btn, IconBtn, NoteRow, PageHeader, Swap } from "@/components/ui";
import { DeleteTemplateModal } from "./DeleteTemplateModal";
import { useTemplateTable } from "./use-template-table";
import { cn } from "@/lib/utils";

/** 超过这个位移才算「要拖」：轻点把手不该把卡抬起来 */
const LIFT_AFTER_PX = 3;

/**
 * 抬起层那三枚按钮禁的是「键盘还能 Tab 进去」（那层已 `aria-hidden`，可聚焦的 child 站在隐形层上），
 * 不是「它们不能按」。所以把 `IconBtn` 自带的 `disabled:opacity-40` 压回原样——
 * 跟手那张必须和流内那张一模一样，他否决过任何让拖影看起来不同的做法（半透明占位同条）。
 */
const LIFT_INERT = "disabled:opacity-100";

/**
 * 抬起那一刻量好的那一列几何（相对列表盒顶）：
 *  - `ids`：其余卡当时的顺序（它们之间整趟不再变），插入位就落在这条序列里；
 *  - `pitch`：一行占的竖向步距（卡高 + 行距）；
 *  - `pad`：半条行距——指针中心落在「上一格的下半 + 行距的一半」之内才算进那一格，
 *    这样格线正好在相邻两格的分界中央，上下两个方向共用同一条线。
 */
interface Slots {
    ids: string[];
    pitch: number;
    pad: number;
}

/** 拖动会话：按下那一刻建，抬起那一刻补几何，整趟只读它 */
interface DragSession {
    id: string;
    /** 指针在卡内的纵向落点 ⇒ 抬起层按它对齐，卡不会在拿起那一瞬跳到指针正中 */
    dy: number;
    height: number;
    left: number;
    width: number;
    startY: number;
    active: boolean;
    /** 抬起那一刻量到的列几何（落位判据唯一的数据源，不再逐帧读 DOM） */
    slots: Slots;
}

export function TemplatesPage() {
    const t = useT();
    const { navigate } = useNavigation();
    const { templates, setTemplates, loaded, commit } = useTemplateTable();
    /** 抬起层（跟手那张）；null = 没在拖 */
    const [lift, setLift] = useState<{ id: string; left: number; width: number } | null>(null);
    const [pendingDelete, setPendingDelete] = useState<ConversionTemplate | null>(null);
    const listRef = useRef<HTMLDivElement | null>(null);
    const dragRef = useRef<DragSession | null>(null);
    /** 已排但还没跑的换位计算（每帧最多一次） */
    const rafRef = useRef(0);
    /** 指针最新落点（盒坐标由 applyOrder 现算），rAF 与松手都读它 */
    const centerRef = useRef(0);
    /** 最近一次算出来的那份 id 顺序；松手要写盘的就是它 */
    const orderRef = useRef("");
    /**
     * 抬起层的 Y 走 motion value 而不是 state：每根 pointermove 都 setState 会把整页重渲染六十次，
     * 而 motion 自己把 transform 写进节点，React 侧一次都不动。
     */
    const liftY = useMotionValue(0);
    /**
     * 会话里三只监听挂在 `window` 上、活过一整趟拖动，所以它们读到的表必须是**当前**那一份：
     * 每 render 把表抄进 ref，拖动路径一律从 ref 读（和 `orderRef` 那条「不认 render 闭包」同一口径）。
     */
    const rowsRef = useRef(templates);
    rowsRef.current = templates;
    /** 这一趟挂在 window 上的监听拆除口（挂的时候顺手记下，收尾与卸载都走它） */
    const offRef = useRef<(() => void) | null>(null);

    /** 拖动期间把光标与文字选中一起按住：只压 cursor 会漏下「一拖选蓝一片」 */
    const setGrabbing = (on: boolean) => {
        document.body.style.cursor = on ? "grabbing" : "";
        document.body.style.userSelect = on ? "none" : "";
    };

    // 拖到一半被别处换走（这一页当场卸掉）⇒ pointerup 不会再来，那根 grabbing 光标会跟着人跑到别的页
    useEffect(
        () => () => {
            offRef.current?.();
            offRef.current = null;
            document.body.style.cursor = "";
            document.body.style.userSelect = "";
        },
        []
    );

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

    /**
     * 抬起那一刻量一次落位几何。这里读 `offsetTop/offsetHeight`（**布局坐标，不吃 transform**），
     * 不读 `getBoundingClientRect`（那是视觉位置，补位弹簧跑着的时候读到的是半路）：
     * 连着拖两下、第二趟在第一趟的弹簧没跑完时抬起 ⇒ 拿「正在飞的线」当边界，落点当场算歪，
     * 那就是他说的「有时无法正常排序」。整套格线记在「相对列表盒顶」的坐标里（盒自身没形变），指针那头
     * 换算到同一套坐标，页面滚动也压不歪它。
     *
     * 卡高恒定是这张卡的设计前提（名称与备注都单行 truncate，见文件头），所以整列是一个等距格子：
     * 步距从相邻两行的 `offsetTop` 差里量（差值把公共原点消掉，也不用管 `offsetParent` 是谁）。
     */
    const measureSlots = (id: string): Slots => {
        const box = listRef.current;
        if (!box) return { ids: [], pitch: 1, pad: 0 };
        const kids = Array.from(box.children) as HTMLElement[];
        const h = kids[0]?.offsetHeight ?? 1;
        const raw = kids.length > 1 ? kids[1].offsetTop - kids[0].offsetTop : 0;
        // 单卡时步距用不上（`at` 恒为 0），但仍要挡掉 0：除法会出 Infinity
        const pitch = raw > 0 ? raw : h + 1;
        const out: string[] = [];
        for (const r of kids) {
            const rid = (r as HTMLElement).dataset.cardId;
            if (rid && rid !== id) out.push(rid);
        }
        return { ids: out, pitch, pad: Math.max(0, (pitch - h) / 2) };
    };

    /** 把手那一按的落点换算到「列表盒」坐标（抬起层对齐用的就是它） */
    const centerAt = (clientY: number, d: DragSession) => clientY - d.dy + d.height / 2;

    /**
     * 指针中心落在第几格 ⇒ 新顺序 = 其余卡（它们之间的相对顺序整趟不变）里插进去。
     * 顺序由缓存的那份格距**算死**，不看当前 state，所以同一个落点反复算都是同一个结果（幂等）。
     * `orderRef` 存最近一次算出的那份 id 顺序：松手时要落盘的正是它，不能等 React 把上一次 setState 刷完。
     */
    const applyOrder = (center: number) => {
        const d = dragRef.current;
        const box = listRef.current;
        if (!d || !box) return;
        const list = rowsRef.current;
        const rel = center - box.getBoundingClientRect().top;
        const { ids, pitch, pad } = d.slots;
        const at = Math.max(0, Math.min(ids.length, Math.floor((rel + pad) / pitch)));
        const next = [...ids.slice(0, at), d.id, ...ids.slice(at)];
        const key = next.join("|");
        if (key === orderRef.current) return;
        // 认不全（这条刚被别处删掉）就当没动：别让一拖把那条卡变没
        if (next.length !== list.length || next.some((x) => !list.some((y) => y.id === x))) return;
        const byId = new Map(list.map((x) => [x.id, x]));
        orderRef.current = key;
        setTemplates(next.map((x) => byId.get(x) as ConversionTemplate));
    };

    const onGripMove = (e: PointerEvent) => {
        const d = dragRef.current;
        if (!d) return;
        if (!d.active) {
            if (Math.abs(e.clientY - d.startY) < LIFT_AFTER_PX) return;
            d.active = true;
            d.slots = measureSlots(d.id);
            orderRef.current = rowsRef.current.map((x) => x.id).join("|");
            setLift({ id: d.id, left: d.left, width: d.width });
            setGrabbing(true);
        }
        // 跟手那一跳走 motion value：它只写一个 transform，不碰 React
        liftY.set(e.clientY - d.dy);
        centerRef.current = centerAt(e.clientY, d);
        // 换位每帧最多算一次：高刷新率鼠标一根线能报上千次 pointermove，逐次 setState 就是逐次重挂投影
        if (!rafRef.current) rafRef.current = requestAnimationFrame(tick);
    };

    const tick = () => {
        rafRef.current = 0;
        applyOrder(centerRef.current);
    };

    /**
     * 松手：先**当场**补算一次落点再收尾——pointerup 常常跑在下一帧之前，只等 rAF 的话最后一格永远少算，
     * 那就是「明明拖过去了却没排上」。然后只在这一步写盘：半途写会把「拖完又拖回来」的中间态留在磁盘上。
     */
    const endGripDrag = () => {
        const d = dragRef.current;
        // 先撤监听：这一趟的 move/up 挂的是这一对闭包，不撤就会一直吃事件
        offRef.current?.();
        offRef.current = null;
        if (rafRef.current) {
            cancelAnimationFrame(rafRef.current);
            rafRef.current = 0;
        }
        // 补算必须在撤会话之前：`applyOrder` 读的就是 `dragRef`，先清它就等于漏掉松手那一格
        if (d?.active) applyOrder(centerRef.current);
        dragRef.current = null;
        setLift(null);
        setGrabbing(false);
        if (!d?.active) return;
        const list = rowsRef.current;
        const ids = (orderRef.current || "").split("|").filter(Boolean);
        const byId = new Map(list.map((x) => [x.id, x]));
        const next = ids
            .map((x) => byId.get(x))
            .filter((x): x is ConversionTemplate => x !== undefined);
        if (next.length !== list.length) return;
        const was = api.peekTemplates() ?? [];
        const changed = was.length !== next.length || next.some((x, i) => x.id !== was[i]?.id);
        if (changed) void commit(next);
    };

    /**
     * 抬起那一按：先建会话，再把 move/up **挂到 `window`**——不挂把手那一行。每换一次位 React 就会在那排
     * 兄弟里搬一次节点，捕获与挂在被搬节点上的监听都跟着没，于是「还按着」被读成「已松手」（文件头第 5 条）。
     */
    const onGripDown = (e: React.PointerEvent<HTMLElement>, id: string) => {
        if (e.button !== 0) return;
        // 上一趟还没收（多指/别处劫走了 up）就先把它的监听撤掉：不然那一对旧闭包永远挂在 window 上
        if (dragRef.current) endGripDrag();
        const card = (e.currentTarget.closest("[data-card-id]") as HTMLElement | null)
            ?.getBoundingClientRect();
        if (!card) return;
        // 压掉这一按的默认动作：不然从把手起步的那一拖会顺手把卡上的文字选中成一片蓝
        e.preventDefault();
        // 捕获仍然要：它买的是「指针滑出窗口也照样收得到 up」。但掉了捕获不算收尾信号，我们只认 up。
        e.currentTarget.setPointerCapture(e.pointerId);
        const d: DragSession = {
            id,
            dy: e.clientY - card.top,
            height: card.height,
            left: card.left,
            width: card.width,
            startY: e.clientY,
            active: false,
            slots: { ids: [], pitch: 1, pad: 0 },
        };
        dragRef.current = d;
        centerRef.current = centerAt(e.clientY, d);
        liftY.set(card.top);
        // 触屏那档 `pointercancel` 之后不会再有 `pointerup`，所以它必须收尾；
        // 鼠标那档发 cancel 多半是系统/原生拖拽抢了手势，别让它把拖影就地弹回。
        const cancel = (ev: PointerEvent) => {
            if (ev.pointerType !== "mouse") endGripDrag();
        };
        // 真·既没 up 也没 cancel 的那一档（手势被整个吞掉）：窗口失焦时收个尾，别把抬起层留在屏上
        const loseFocus = () => endGripDrag();
        window.addEventListener("pointermove", onGripMove);
        window.addEventListener("pointerup", endGripDrag);
        window.addEventListener("pointercancel", cancel);
        window.addEventListener("blur", loseFocus);
        offRef.current = () => {
            window.removeEventListener("pointermove", onGripMove);
            window.removeEventListener("pointerup", endGripDrag);
            window.removeEventListener("pointercancel", cancel);
            window.removeEventListener("blur", loseFocus);
        };
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

                        <div ref={listRef} className="flex flex-col gap-4">
                            {templates.map((tpl) => (
                                <motion.div
                                    key={tpl.id}
                                    data-card-id={tpl.id}
                                    layout="position"
                                    initial={{ opacity: 0 }}
                                    animate={{ opacity: 1 }}
                                    transition={{
                                        ...REFLOW,
                                        // 淡入不跟着弹簧走：弹簧是给位移用的，opacity 过冲只读成闪烁
                                        opacity: { duration: 0.18, ease: "easeOut" },
                                    }}
                                >
                                    <TemplateRow
                                        template={tpl}
                                        dragging={dragId === tpl.id}
                                        onOpen={() => navigate("template", { templateId: tpl.id })}
                                        onCopy={() => duplicate(tpl)}
                                        onDelete={() => setPendingDelete(tpl)}
                                        onGripDown={(e) => onGripDown(e, tpl.id)}
                                    />
                                </motion.div>
                            ))}
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
              跟手那层：同一张卡的重绘件（不是 DOM 克隆——HTML5 那套位图快照在这里用不上，
              pointer 路径下我们手里就有数据）。宽度钉死在按下那一刻量到的值，所以它在流外也不换行、
              不跟着右栏伸缩；`pointer-events-none` 让它不吃命中，命中全交给下面真正在流的卡。
              **必须 portal 到 body**：这一页挂在 `motion.main` 里，那块入场演的是 `y`（transform ⇒ 它成了
              fixed 后代的包含块，视口原点当场挪到它身上），还带 `overflow-auto`（fixed 后代照样被它裁）。
              口径同 App.tsx:110 那句「垃圾桶为什么留在 Shell 层」——别赌 fixed 能不能穿过滚动容器。
            */}
            {lift &&
                liftTpl &&
                createPortal(
                    <div
                        className="pointer-events-none fixed left-0 top-0 z-[999]"
                        aria-hidden="true"
                    >
                        <motion.div style={{ x: lift.left, y: liftY, width: lift.width }}>
                            <TemplateRow template={liftTpl} overlay />
                        </motion.div>
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
    dragging,
    overlay,
    onOpen,
    onCopy,
    onDelete,
    onGripDown,
}: {
    template: ConversionTemplate;
    /** 留在流里的那张：只点亮描边，不加透明度（稿内定案） */
    dragging?: boolean;
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
                overlay
                    ? "border-accent shadow-lg"
                    : dragging
                      ? "border-accent"
                      : "border-stroke"
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

/** 占位照真实卡排（同一套 p-5 / 36 图标盒 / 双行文字高度）：读数到手时高度几乎不动 */
function LoadingTemplates() {
    return (
        <div className="flex flex-col gap-4">
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

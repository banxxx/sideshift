/**
 * 任务列表页：吸顶页头（标题 + 统计副标 + 分段筛选）→ 任务卡列表（三行卡：状态盒/进度/明细+按钮组）→ 两套空态。
 * 空态必须在首轮读数到手后（`loaded` 闸门）才允许出现，否则会闪一张「这里什么都没有」。
 * 删除是一段可反悔的动作：卡片克隆沿弧线飞进垃圾桶，`lead` 时刻才摘出列表并通知后端，落地前点它或 Esc 都算追回（曲线见 delete-flight.ts）。
 * 换筛选按换页语义处理：在场每张卡一律从下方同一条曲线升起、逐卡错峰（entry-curve.ts），谁都不回原格子；被筛掉的当场消失，不做退场层。
 */
import { ArrowRight, Check, Download, Inbox, RefreshCw, SearchX, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { t, tSource, useT, type TranslateFn } from "@/lib/i18n";
import { errOf } from "@/lib/errors";
import { useNavigation } from "@/lib/navigation";
import { BAR_COLOR, needsNetwork, progressChip, runCounts, stageLabel } from "@/lib/rail-view";
import { formatDuration, formatElapsed, formatSize, loaderLabel, outputNameOf, truncateMiddle } from "@/lib/format";
import {
    commitDelete,
    restoreDeleted,
    stageDeleted,
    syncTrash,
    unstageDeleted,
    useTasksReloadTick,
    useTrash,
} from "@/lib/trash-store";
import type { ConversionTask, TaskStatus } from "@/lib/types";
import { Bar, Btn, PageHeader, Panel, SegTabs, Swap, ToneChip } from "@/components/ui";
import { cn, prefersReducedMotion } from "@/lib/utils";
import { REFLOW } from "@/lib/springs";
import { abortAllFlights, startFlight, type Flight } from "./delete-flight";
import { entryStepMs, playEntry } from "@/lib/entry-curve";
import { useScrollResetOn } from "@/lib/page-scroll";

type Filter = "all" | "running" | "success" | "failed";

/** 只给最靠前几张排错峰，第 6 张之后再等就成「卡片在原地迟到了」 */
const STAGGER_STEP = 0.026;

/** 状态 → 图标盒配色与图标（36×36 r10，$*-dim 底 + 17px 图标） */
const CARD_ICON: Record<TaskStatus, { icon: typeof Check; box: string; fg: string }> = {
    queued: { icon: Download, box: "bg-surface-2", fg: "text-text-3" },
    running: { icon: Download, box: "bg-gold-dim", fg: "text-gold" },
    success: { icon: Check, box: "bg-emerald-dim", fg: "text-emerald" },
    failed: { icon: X, box: "bg-redstone-dim", fg: "text-redstone" },
    cancelled: { icon: X, box: "bg-surface-2", fg: "text-text-3" },
};

const FILTERS = [
    { key: "all" as const },
    { key: "running" as const },
    { key: "success" as const },
    { key: "failed" as const },
];

/** 页签文案过 `t`、value 保持 id：表建在函数里，切语言时随组件重渲染重建 */
const filterTabs = (t: TranslateFn): Array<{ key: Filter; label: string }> =>
    FILTERS.map((f) => ({
        key: f.key,
        label:
            f.key === "all"
                ? t("common.entry-2", "全部")
                : f.key === "running"
                  ? t("tasks.running", "运行中")
                  : f.key === "success"
                    ? t("lib.entry-2", "已完成")
                    : t("lib.failed", "失败"),
    }));

/** 状态 → 主按钮文案：同一档位同一个动作（进详情），只有落点读的内容随状态换 */
const primaryLabel = (t: TranslateFn): Record<TaskStatus, string> => ({
    queued: t("tasks.view-progress", "查看进度"),
    running: t("tasks.view-progress", "查看进度"),
    success: t("tasks.view-report", "查看报告"),
    failed: t("tasks.view-details", "查看详情"),
    cancelled: t("tasks.view-details", "查看详情"),
});

/** 快照按创建时间倒序一份：首帧（缓存初值）与每趟轮询回来走同一条排序，别两处各写一遍 */
const byNewest = (list: ConversionTask[]) => [...list].sort((a, b) => b.createdAt - a.createdAt);

export function TasksPage() {
    const t = useT();
    // 首帧直接吃上一趟读数（`api.peekTasks`，理由写在那儿）：换页会重挂载，
    // 空着挂载就是「占位层叠在列表上换一次」，正压在整页淡入的途中
    const [tasks, setTasks] = useState<ConversionTask[]>(() => byNewest(api.peekTasks() ?? []));
    /** 第一轮读数到手前不许宣布「没有任务」：`listTasks()` 是异步的，空态与列表都由它把关，
     *  否则每次进这一页都会先闪一张空壳、再蹦出列表（换页那 260ms 入场正好把它放大）。 */
    const [loaded, setLoaded] = useState(() => api.peekTasks() !== null);
    const [filter, setFilter] = useState<Filter>("all");
    /** 飞行中：卡片本体隐身但**保住槽位**（视觉交给 overlay 里的克隆） */
    const [flying, setFlying] = useState<Set<string>>(new Set());
    /** 已摘除：lead 时刻起不占位、不计数（与回收站取交集后才真正生效，见下面 gone 的说明） */
    const [omitted, setOmitted] = useState<Set<string>>(new Set());
    const trash = useTrash();
    const flights = useRef(new Map<string, Flight>());
    const tick = useTasksReloadTick();

    // 1s 轮询：mock 引擎推进与后端任务态切换都靠它刷新；回收站的撤回也走它（tick 变化立即重建一次）
    useEffect(() => {
        const load = () =>
            api
                .listTasks()
                .then((list) => setTasks(byNewest(list)))
                // 读失败按空列表处理，但照样算"读到了"：闸门一直压着会让页面卡在占位上，
                // 比报一次空更糟。1s 轮询会在下一趟把真实数据换回来。
                .catch(() => setTasks([]))
                .finally(() => setLoaded(true));
        void load();
        const timer = window.setInterval(load, 1000);
        return () => window.clearInterval(timer);
    }, [tick]);

    // 回收站只活在 Rust 进程内存里：webview 刷新（进程没退）后靠这一次对账把它读回来
    useEffect(() => {
        void syncTrash();
        return abortAllFlights;
    }, []);

    /**
     * 换筛选是一次换页，不是一次重排：在场每张卡片一律从下方升起（@/lib/entry-curve.ts 那条「送 + 弹」曲线，
     * 逐卡错峰），谁都不回自己原来的格子。弹簧会老老实实把留下来的卡片送回旧格子，正好把换页语义打掉，
     * 而且过冲按行程等比（挪 900px 弹 30px、挪 20px 弹 0.7px），两种切换统一不了手感——所以这一拍里
     * 不能有活的投影节点，做法见下面列表容器上的 key={filter}。
     * 投影从此恒开，不再分「关掉一整拍再交还」：曲线演完不用归还什么，删除补位随时都有弹簧可用。
     * 被筛掉的卡片当场从 DOM 消失：下落退场层试过，那张克隆会盖在页面上，已按用户判定整体删掉。
     * useLayoutEffect 是必需的：effect 晚了就是「先画出新内容、再把它抹到 0」，看着像闪一下。
     * 首屏不跑（交给卡片挂载时 motion 自己的 initial 淡入）。
     */
    const bodyRef = useRef<HTMLDivElement | null>(null);
    // 换档是一次换页 ⇒ 滚动跟着归顶（容器归 App 的 main 常驻，不清就会带着上一档的行数进来）
    useScrollResetOn(filter);
    const firstPaint = useRef(true);
    useLayoutEffect(() => {
        if (firstPaint.current) {
            firstPaint.current = false;
            return;
        }
        // 曲线写内层：外层的 transform 归 layout 投影管（删除补位那一路），两层各写各的才不抢属性
        const targets = Array.from(bodyRef.current?.querySelectorAll<HTMLElement>("[data-entry]") ?? []);
        // 筛到空档时没有卡片可演，就让整块淡入一下，别让空态凭空出现
        const nodes = targets.length ? targets : bodyRef.current ? [bodyRef.current] : [];
        const step = prefersReducedMotion() ? 0 : entryStepMs(nodes.length);
        const undo = nodes.map((el, i) => playEntry(el, i * step));
        return () => undo.forEach((stop) => stop());
    }, [filter]);

    /**
     * 摘除名单要再过一道「回收站里还在吗」：只按黑名单压，撤回那条路会漏——轮询把任务读
     * 回来了，黑名单还压着它，卡片就再也见不着。现在黑名单只在「仍在回收站里」或「还在飞」
     * 时生效，撤回/清空一落地就自动放行，两端不可能对不上账。
     */
    const trashed = new Set(trash.map((e) => e.taskId));
    const gone = new Set(
        [...omitted].filter((id) => trashed.has(id) || flights.current.has(id))
    );
    const left = tasks.filter((t) => !gone.has(t.id));
    /** 页头统计跟着收：正在飞的卡片也不该再挂在计数里等下一拍轮询 */
    const shown = left.filter((t) => !flying.has(t.id));
    const visible = shown.filter((t) => matchesFilter(filter, t));

    /** 切页签只改 filter：这一趟位移交给入场曲线，投影由列表容器的 key 让路（见下方列表）。 */
    const changeFilter = (next: Filter) => setFilter(next);

    const toggle = (
        setter: (fn: (prev: Set<string>) => Set<string>) => void,
        id: string,
        on: boolean
    ) =>
        setter((prev) => {
            if (prev.has(id) === on) return prev;
            const next = new Set(prev);
            if (on) next.add(id);
            else next.delete(id);
            return next;
        });
    const setFlight = (id: string, on: boolean) => toggle(setFlying, id, on);
    const setGone = (id: string, on: boolean) => toggle(setOmitted, id, on);

    /**
     * 一条删除的完整流程。两个入口都收在这里，所以「按钮删的」和「键盘删的」行为与可反悔窗口完全一致。
     */
    const beginDelete = (task: ConversionTask) => {
        if (flights.current.has(task.id)) return;
        const el = cardEl(task.id);
        if (!el) return;
        /** 追回时要知道「后端是否已经收到删除」：收了就得走撤回，而不是假装什么都没发生 */
        let committed = false;
        let settled: Promise<void> = Promise.resolve();
        let failure: unknown = null;

        // 先让垃圾桶出现：飞行落点是量出来的，不是猜的
        stageDeleted(task);
        setFlight(task.id, true);

        const flight = startFlight(el, () => {
            committed = true;
            setGone(task.id, true);
            settled = commitDelete(task.id).catch((e) => {
                failure = e;
            });
        });
        flights.current.set(task.id, flight);

        void flight.done.then((outcome) => {
            flights.current.delete(task.id);
            setFlight(task.id, false);
            if (outcome === "drop") {
                // 后端若拒绝了这次删除，把卡片放回去并说清楚——静默回退最查不出来
                void settled.then(() => {
                    if (!failure) return;
                    setGone(task.id, false);
                    unstageDeleted(task.id);
                    notify(
                        t("tasks.couldn-delete", "删除任务失败：{{reason}}", {
                            reason: errOf(failure),
                        }),
                        "error"
                    );
                });
                return;
            }
            // lead 之后才追回 = 删除已提交，只能原样撤回；lead 之前追回 = 什么都没发生
            if (committed) void restoreAfterCommit(task.id);
            else unstageDeleted(task.id);
        });
    };

    return (
        <div className="flex flex-col gap-5 pb-6">
            {/* 吸顶页头：本页是全站唯一会长到好几屏的页面，筛选页签必须一直够得着。
                留白这圈由**吸顶盒自己**出（pt-6），main 不再有纵向 padding、这里也不许用负 margin：
                负 margin 会把盒子顶到 containing block（页根 div 的内容盒上沿）之外，浏览器原地把它
                夹回来——静止时页头底色压住列表 24px，滚动时顶部又永远留一条盖不住的带子。
                盒子贴在滚动容器上沿贴合，卡片是从它**底下**穿过去的。
                pb-5 + -mb-5 抵掉 flex 的 gap-5：静止观感与不吸顶时逐像素相同，但那 20px 归底色管。
                纯色底平时看不出来（页面底同色），只有卡片从底下滚过来才显形——不额外加描边，
                本项目的页头没有分隔线。z-20 是必需的：卡片带 transform 就是层叠上下文，
                按文档顺序会盖在页头上面。 */}
            <div className="sticky top-0 z-20 -mb-5 bg-background pt-6 pb-5">
                <PageHeader
                    compact
                    title={t("tasks.conversion-tasks", "转换任务")}
                    sub={
                        !loaded
                            ? t("tasks.loading-tasks", "正在读取任务…")
                            : tasks.length === 0
                              ? t("tasks.pick-modpack", "从首页选择整合包，开始第一次转换")
                              : summary(shown)
                    }
                    subTone="mono"
                    right={<SegTabs items={filterTabs(t)} value={filter} onChange={changeFilter} />}
                />
            </div>

            {/* 骨架 → 卡片/空态 是同位换批（首帧读档落地那一刻整块换掉），
                走全站共用的一对进出节拍（SWAP：进 180ms 落 6px / 出 90ms 溶解）。
                筛选结果为空 ↔ 有卡片 这一档**不**套 Swap：卡片这层的进出场由 entry-curve 那条
                「送+弹」曲线管，两层退场叠在同一片区域只会互相盖（廿九轮的判定）。 */}
            <Swap swapKey={!loaded ? "loading" : tasks.length === 0 ? "empty" : "list"}>
                {!loaded ? (
                    <LoadingTasks />
                ) : tasks.length === 0 ? (
                    <EmptyTasks />
                ) : (
                    /* 常驻容器：卡片节点在同一档筛选里长期存续，删除补位的行程由 motion 的 layout 弹簧做。
                       为什么不用 AnimatePresence 换 key：那会让旧内容先卸载/后卸载，两种都有代价——mode="wait"
                       中间塌一次高度（滚到下面切页签会跳），mode="popLayout" 则新旧同 id 卡片重叠，旧的 ref
                       清理会把新登记的那个节点删掉，删除飞行拿不到矩形。

                       key={filter} 是必需的，不是冗余：motion 只在**创建投影节点那一次**读 layout 属性
                       （use-visual-element 里 createProjectionNode 受 `!visualElement.projection` 把关，
                       此后 props.layout 再怎么变都不回填）。所以以前那种「切页签关掉投影、演完再打开」会让
                       这一拍里重新挂载的卡片**永远**没有投影——切回来再删就是「其他卡片直接归位、没有弹力」。
                       现在投影恒开；换页签那拍要让位给入场曲线，就把整列重挂载：新挂载的节点没有历史快照，
                       checkUpdate 走 notifyAnimationStart 而不是播放，谁都不会被弹簧送回旧格子。
                       ref 用回调登记：key 一变 React 先走旧树的卸载（置 null）再挂新树，直写 current 会留着悬空节点。 */
                    <div
                        key={filter}
                        ref={(el) => {
                            bodyRef.current = el;
                        }}
                        className="flex flex-col gap-4"
                    >
                        {visible.length === 0 ? (
                            <EmptyFilter />
                        ) : (
                            visible.map((t, i) => (
                                <TaskRow
                                    key={t.id}
                                    task={t}
                                    index={i}
                                    ghosted={flying.has(t.id)}
                                    onDelete={beginDelete}
                                />
                            ))
                        )}
                    </div>
                )}
            </Swap>
        </div>
    );
}

/** 卡片根节点登记表：飞行要拿真实矩形，克隆和落点都基于它，不用 querySelector 猜 DOM */
const cardEls = new Map<string, HTMLElement>();
const cardEl = (id: string) => cardEls.get(id);

/** lead 之后才追回：删除已经提交到后端，只能原样撤回（暂存没动过，放回即可） */
async function restoreAfterCommit(id: string) {
    try {
        await restoreDeleted(id);
        notify(t("tasks.restored-task", "已追回，任务回到列表"), "info");
    } catch (e) {
        notify(t("tasks.couldn-restore-reason", "追回失败：{{reason}}", { reason: errOf(e) }), "error");
    }
}

/** 页头统计副标：运行中 N · 已完成 N · 失败 N */
function summary(tasks: ConversionTask[]): string {
    return t("tasks.running-running", "运行中 {{running}} · 已完成 {{success}} · 失败 {{failed}}", {
        running: countOf(tasks, "running"),
        success: countOf(tasks, "success"),
        failed: countOf(tasks, "failed"),
    });
}

/** 四档筛选覆盖五个状态：排队/运行归「运行中」，取消归「失败」（都是没跑成）。 */
function matchesFilter(filter: Filter, t: ConversionTask): boolean {
    if (filter === "all") return true;
    if (filter === "running") return t.status === "running" || t.status === "queued";
    if (filter === "failed") return t.status === "failed" || t.status === "cancelled";
    return t.status === filter;
}

function countOf(tasks: ConversionTask[], filter: Exclude<Filter, "all">): number {
    return tasks.filter((t) => matchesFilter(filter, t)).length;
}

/* ---------------- 首轮读数占位 / 空态：整页无任务（YBR4K）/ 某一档筛完没有 ---------------- */

/** 占位照真实卡的骨架排（同一张 Panel、同样的两行文字高度）：读数到手时高度几乎不动，
 *  不会看到"矮一截又长回来"。这里不挂 motion —— 它演的是"还没有内容"，不是内容入场。 */
function LoadingTasks() {
    return (
        <div className="flex flex-col gap-4">
            {Array.from({ length: 3 }, (_, i) => (
                <Panel key={i} gap={12}>
                    <div className="flex w-full items-center gap-3">
                        <span className="size-9 shrink-0 animate-pulse rounded-[10px] bg-surface-2" />
                        <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                            <span className="h-[13px] w-40 animate-pulse rounded bg-stroke" />
                            <span className="h-[10px] w-56 animate-pulse rounded bg-stroke-soft" />
                        </div>
                        <span className="h-[22px] w-16 shrink-0 animate-pulse rounded-full bg-stroke" />
                        <span className="h-[11px] w-8 shrink-0 animate-pulse rounded bg-stroke-soft" />
                    </div>
                    <div className="flex w-full items-center justify-between gap-3">
                        <span className="h-[11px] w-52 animate-pulse rounded bg-stroke-soft" />
                        <span className="h-8 w-24 shrink-0 animate-pulse rounded-lg bg-stroke" />
                    </div>
                </Panel>
            ))}
        </div>
    );
}

/** 空态块高度：70vh 在 1200×800 基准下正好等于设计稿的 560；窗口变矮先收这里（下限 400 保证
 *  图标盒+两行文案+按钮 176 的内容不破版），变高封顶 640，免得拉成一整屏空白。 */
function EmptyTasks() {
    const t = useT();
    const { switchPrimary } = useNavigation();
    return (
        <div className="flex h-[clamp(400px,70vh,640px)] flex-col items-center justify-center gap-4 rounded-[12px] bg-bg-app px-5 py-10">
            <div className="flex flex-col items-center gap-4">
                <span className="flex size-14 items-center justify-center rounded-2xl bg-surface-2">
                    <Inbox className="size-6 text-text-3" />
                </span>
                <div className="flex flex-col items-center gap-1">
                    <span className="font-mono text-[16px] leading-[24px] font-semibold text-text-1">
                        {t("tasks.conversion-tasks-yet", "还没有转换任务")}
                    </span>
                    <span className="font-mono text-[12px] leading-[18px] font-normal text-text-3">
                        {t("tasks.upload-minecraft", "上传 Minecraft 整合包后，转换任务会出现在这里")}
                    </span>
                </div>
            </div>
            <Btn
                variant="primary"
                className="border border-stroke text-[12px] font-medium"
                onClick={() => switchPrimary("home")}
            >
                {t("tasks.select-modpack", "去首页选择整合包")}
            </Btn>
        </div>
    );
}

/** 某一档筛完没有任务：任务其实有，只是这一档没有，所以图标与文案都得跟「整页空」分开。
 *  照旧做成居中的块（水平+垂直都居中），不留一行飘着的裸文字；比整页空态矮一档，
 *  免得切个页签就换来一大片空白。出现时的淡入归整块内容那一次 fade-through，这里不再各自动画。 */
function EmptyFilter() {
    const t = useT();
    return (
        <div className="flex h-[clamp(240px,38vh,380px)] flex-col items-center justify-center gap-4 rounded-[12px] bg-bg-app px-5 py-10">
            <span className="flex size-14 items-center justify-center rounded-2xl bg-surface-2">
                <SearchX className="size-6 text-text-3" />
            </span>
            <div className="flex flex-col items-center gap-1">
                <span className="font-mono text-[16px] leading-[24px] font-semibold text-text-1">
                    {t("tasks.tasks-filter", "该筛选下暂无任务")}
                </span>
                <span className="font-mono text-[12px] leading-[18px] font-normal text-text-3">
                    {t("tasks.switch-convert", "切回「全部」看看，或到首页再转换一个整合包")}
                </span>
            </div>
        </div>
    );
}

/* ---------------- 卡片行：键盘删除与补位弹簧挂在这一层 ---------------- */

/**
 * motion.div 只管「这张卡在列表里的位置」，卡面仍是 TaskCard。
 * `layout="position"`：补位只平移不缩形（等高弹簧的等价物），并且**恒定开着**——投影属性只在
 * 节点创建时被 motion 读一次，中途关过就再也开不回来了（根因见列表容器上那段注释）。
 * 位移曲线挂在**内层** `data-entry` 节点上：外层的 transform 归投影管（删除补位时它正写着 translateY），
 * 两层各写各的才不会抢同一个属性。
 * `initial/animate` 那记淡入仍留着：它管的是「第一次挂载」（进页面、从回收站恢复），
 * 与换页签的统一入场是两条时间线，叠在一起都是淡入，不冲突。
 */
function TaskRow({
    task,
    index,
    ghosted,
    onDelete,
}: {
    task: ConversionTask;
    index: number;
    /** 正在飞：本体隐身保位，画面交给 overlay 里的克隆 */
    ghosted: boolean;
    onDelete: (task: ConversionTask) => void;
}) {
    const deletable = task.status !== "running" && task.status !== "queued";
    const delay = prefersReducedMotion() ? 0 : Math.min(index, 5) * STAGGER_STEP;
    const fly = () => {
        if (deletable) onDelete(task);
    };

    return (
        <motion.div
            ref={(el) => {
                if (el) cardEls.set(task.id, el);
                else cardEls.delete(task.id);
            }}
            layout="position"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{
                ...REFLOW,
                // 错峰按可见序号给；reduced-motion 下弹簧本来就被收掉，不必再排队
                delay,
                // 淡入不跟着弹簧走：弹簧是给位移用的，opacity 过冲只会让卡片在满透明度上停一下
                opacity: { duration: 0.18, delay, ease: "easeOut" },
            }}
            tabIndex={deletable ? 0 : -1}
            onKeyDown={(e) => {
                if (e.key === "Delete" || e.key === "Backspace") {
                    e.preventDefault();
                    fly();
                }
            }}
            className={cn(
                // 焦点环留得住：卡片可聚焦才有「键盘也能删」这条路
                "rounded-[12px] outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
                ghosted && "invisible"
            )}
        >
            <div data-entry>
                <TaskCard task={task} onDelete={fly} />
            </div>
        </motion.div>
    );
}

/* ---------------- 任务卡 ---------------- */

function TaskCard({ task, onDelete }: { task: ConversionTask; onDelete: () => void }) {
    const t = useT();
    const { navigate } = useNavigation();
    const running = task.status === "running" || task.status === "queued";
    const icon = CARD_ICON[task.status];
    const Icon = icon.icon;
    const chip = progressChip(task);
    const started = task.startedAt ?? task.createdAt;
    const elapsed = (task.finishedAt ?? Date.now()) - started;
    const outName = task.outputFileName ?? outputNameOf(task.pack.fileName);

    /** 重试是原地动作（同一 id 重新排队）：列表 1s 轮询会把这行读成运行中，不该顺手压一层详情页 */
    const retry = async () => {
        const res = await api.retryTask(task.id);
        if (!res) return;
        if (res.queued) notify(t("tasks.conversion-running", "已有转换正在进行，重试任务已加入队列"), "info");
    };

    const cancel = async () => {
        await api.cancelTask(task.id);
        notify(t("tasks.task-canceled-downloaded", "任务已取消，已下载的文件保留在缓存"), "info");
    };

    /** 打开产物所在目录。此前两处 `void` 把 openPath 的 reject 吞掉了，表现为「点了没反应」；
     *  现在失败一律走 notify 并带上错误原文。路径口径同时改为优先用后端回传的真实产物路径
     *  outputPath（旧记录缺该字段时才按输出目录 + 文件名重建），不再凭猜测拼路径。 */
    const openOutput = async () => {
        try {
            const p =
                task.outputPath ??
                (await api.resolveOutputPath(outName, task.options.outputOverride));
            await api.openDir(api.dirOf(p));
        } catch (e) {
            notify(
                t("tasks.couldn-open", "打开输出目录失败：{{reason}}", { reason: errOf(e) }),
                "error"
            );
        }
    };

    return (
        <Panel gap={12}>
            {/* row1：图标盒 + 名称两行 + 状态芯片 + 耗时 */}
            <div className="flex w-full items-center gap-3">
                <span
                    className={cn(
                        "flex size-9 shrink-0 items-center justify-center rounded-[10px]",
                        icon.box
                    )}
                >
                    <Icon className={cn("size-[17px]", icon.fg)} />
                </span>
                <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                    <span className="truncate font-mono text-[13px] leading-[20px] font-semibold text-text-1">
                        {truncateMiddle(task.pack.fileName, 32)}
                    </span>
                    {/* 副标那枚「指向产物」的箭头原来是字符 `→`：等宽字里它跟着字距走、
                        粗细和旁边图标盒那批 lucide 线条也对不上。换成图标后两处同源。 */}
                    <span className="flex min-w-0 items-center gap-1.5 font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        <ArrowRight className="size-3 shrink-0" strokeWidth={2.5} />
                        <span className="truncate">{subLine(task, outName)}</span>
                    </span>
                </div>
                <ToneChip tone={chip.tone} dot={task.status === "running"}>
                    {chipLabel(task, chip.label)}
                </ToneChip>
                <span className="shrink-0 font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {running ? formatElapsed(elapsed) : formatDuration(elapsed)}
                </span>
            </div>

            {/* row2：进度条（仅运行中） */}
            {running && (
                <div className="flex w-full items-center gap-2.5">
                    <Bar percent={task.progress} className="bg-bg-app" fillClass={BAR_COLOR[task.status]} />
                    <span className="w-9 shrink-0 text-right font-mono text-[11px] leading-[16px] font-semibold text-gold">
                        {task.progress}%
                    </span>
                </div>
            )}

            {/* 失败态：错误盒（$bg-app + $redstone-dim 描边） */}
            {task.status === "failed" && task.error && (
                <div className="w-full rounded-lg border border-redstone-dim bg-bg-app px-4 py-3">
                    <span className="break-words font-mono text-[11px] leading-[16px] font-normal text-redstone">
                        {tSource(task.error.title)} · {errOf(task.error.code ?? task.error.detail)}
                    </span>
                </div>
            )}

            {/* row3：明细行 + 操作 */}
            <div
                className={cn(
                    "flex w-full items-center",
                    task.status === "failed" ? "justify-end gap-2" : "justify-between gap-3"
                )}
            >
                {task.status !== "failed" && (
                    <span className="min-w-0 flex-1 truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        {detailLine(task)}
                    </span>
                )}
                <div className="flex shrink-0 items-center gap-2">
                    {/* 槽位固定：删除（最左）→ 状态动作 → 主按钮。
                        主按钮永远占最右一档、只管进详情，文案随状态换；
                        运行/排队的行后端拒绝删除，这一档整枚不出现。 */}
                    {!running && (
                        <Btn variant="danger" size="sm" onClick={() => onDelete()}>
                            {t("tasks.delete", "删除")}
                        </Btn>
                    )}
                    {running && (
                        <Btn size="sm" onClick={() => void cancel()}>
                            {t("common.cancel", "取消")}
                        </Btn>
                    )}
                    {task.status === "success" && (
                        <Btn size="sm" onClick={() => void openOutput()}>
                            {t("tasks.open-output", "打开输出位置")}
                        </Btn>
                    )}
                    {(task.status === "failed" || task.status === "cancelled") && (
                        <Btn size="sm" icon={RefreshCw} onClick={() => void retry()}>
                            {task.status === "failed" ? t("tasks.retry", "重试") : t("tasks.reconvert", "重新转换")}
                        </Btn>
                    )}
                    <Btn
                        variant="primary"
                        size="sm"
                        className="font-semibold"
                        onClick={() =>
                            navigate("task", {
                                taskId: task.id,
                                // 已完成直达结果，其余落概况：进详情页不必多点一次签
                                tab: task.status === "success" ? "result" : "overview",
                            })
                        }
                    >
                        {primaryLabel(t)[task.status]}
                    </Btn>
                </div>
            </div>
        </Panel>
    );
}

/** row1 副标（箭头由渲染处给出）：输出名（运行/成功）或加载器 + 中断阶段（失败） */
function subLine(task: ConversionTask, outName: string): string {
    if (task.status === "failed") {
        return `${loaderLabel(task.pack.loader)} ${task.options.mcVersion} · ` + t("tasks.stopped-stage", "中断于{{stage}}阶段", {
            stage: stageLabel(task.stage ?? "builder", needsNetwork(task)),
        });
    }
    if (task.status === "success" && task.outputSizeBytes != null) {
        return `${outName} · ${formatSize(task.outputSizeBytes)}`;
    }
    return outName;
}

/** 状态芯片文案：取件阶段追加 "7/12" 计数（联网任务按「已下载/需联网」，纯本地按「已取件/全部」） */
function chipLabel(task: ConversionTask, base: string): string {
    if (task.status === "running" && task.stage === "downloader" && task.total != null) {
        return `${base} ${runCounts(task)}`;
    }
    return base;
}

/** row3 左侧明细行 */
function detailLine(task: ConversionTask): string {
    if (task.status === "running" || task.status === "queued") {
        const raw = task.logs[task.logs.length - 1]?.message;
        const where = task.stage ? `${stageLabel(task.stage, needsNetwork(task))} ·` : `${t("tasks.queued", "排队")} ·`;
        return [where, raw === undefined ? t("tasks.waiting", "等待开始…") : tSource(raw)].filter(Boolean).join(" ");
    }
    if (task.status === "success") {
        const c = task.counts;
        return c
            ? t("tasks.remove-client", "剔除 {{remove}} 个客户端专属模组 · 补齐 {{add}} 个服务端依赖", {
                  remove: c.remove,
                  add: c.add,
              })
            : t("tasks.conversion-complete", "转换完成");
    }
    return t("tasks.task-canceled", "任务已取消 · 已取回的文件保留在下载缓存");
}

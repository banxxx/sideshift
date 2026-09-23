/**
 * 任务列表页 Tasks（SS.pen `YUUJQ`，空态 `YBR4K`）
 *
 * 结构：页头（标题 + 等宽统计副标 + 右侧分段筛选）→ 任务卡列表（gap16）。
 * 单卡（gap12 padding20）三行：
 *  row1 = 36×36 状态图标盒 + 包名/输出名两行 + 状态芯片 + 右侧耗时
 *  row2 = 进度条 + 百分比（仅运行中）
 *  row3 = 左等宽明细行 + 右操作按钮组（槽位固定：删除最左 → 状态动作 → 主按钮进详情，
 *         失败态改为错误盒 + 右对齐按钮；详情页是任务唯一的下钻目的地，列表不再各发各的跳转）
 * 空态为 560 高无边框块：56×56 图标盒 + 两行等宽文案 + accent 主按钮。
 *
 * 删除不是「点一下就没」，而是一段可反悔的动作（曲线与节奏见 ./delete-flight.ts）：
 * 两个入口（删除按钮 / 卡片聚焦后按 Delete）走同一条流程 ——
 * 卡片克隆出去沿弧线飞进右下角垃圾桶，`lead` 时刻才从列表摘掉并通知后端（下方卡片用 motion 的
 * layout 弹簧补位），落地前点它或按 Esc 都算追回，数据没动过。
 */
import { Check, Download, Inbox, RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { motion, type Transition } from "motion/react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
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
import { Bar, Btn, PageHeader, Panel, SegTabs, ToneChip } from "@/components/ui";
import { cn } from "@/lib/utils";
import {
    abortAllFlights,
    prefersReducedMotion,
    startFlight,
    type Flight,
} from "./delete-flight";

type Filter = "all" | "running" | "success" | "failed";

/** 补位弹簧（样片实测 k=340 / c=26）：位移大的自己跑得快，到位时间近似恒定
 *  （实测 40px≈367ms、466px≈467ms）；错峰靠 delay 排出一道波，而不是靠快慢差。 */
const REFLOW: Transition = { type: "spring", stiffness: 340, damping: 26, mass: 1 };
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
    { key: "all" as const, label: "全部" },
    { key: "running" as const, label: "运行中" },
    { key: "success" as const, label: "已完成" },
    { key: "failed" as const, label: "失败" },
];

/** 状态 → 主按钮文案：同一档位同一个动作（进详情），只有落点读的内容随状态换 */
const PRIMARY_LABEL: Record<TaskStatus, string> = {
    queued: "查看进度",
    running: "查看进度",
    success: "查看报告",
    failed: "查看详情",
    cancelled: "查看详情",
};

export function TasksPage() {
    const [tasks, setTasks] = useState<ConversionTask[]>([]);
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
                .then((list) => setTasks([...list].sort((a, b) => b.createdAt - a.createdAt)))
                .catch(() => setTasks([]));
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
                        `删除任务失败：${failure instanceof Error ? failure.message : String(failure)}`,
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
        <div className="flex flex-col gap-5">
            <PageHeader
                compact
                title="转换任务"
                sub={tasks.length === 0 ? "从首页选择整合包，开始第一次转换" : summary(shown)}
                subTone="mono"
                right={<SegTabs items={FILTERS} value={filter} onChange={setFilter} />}
            />

            {tasks.length === 0 ? (
                <EmptyTasks />
            ) : (
                <div className="flex flex-col gap-4">
                    {visible.map((t, i) => (
                        <TaskRow
                            key={t.id}
                            task={t}
                            index={i}
                            ghosted={flying.has(t.id)}
                            onDelete={beginDelete}
                        />
                    ))}
                    {visible.length === 0 && (
                        <p className="py-10 text-center font-mono text-[11px] text-text-3">
                            该筛选下暂无任务
                        </p>
                    )}
                </div>
            )}
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
        notify("已追回，任务回到列表", "info");
    } catch (e) {
        notify(`追回失败：${e instanceof Error ? e.message : String(e)}`, "error");
    }
}

/** 页头统计副标：运行中 N · 已完成 N · 失败 N */
function summary(tasks: ConversionTask[]): string {
    return `运行中 ${countOf(tasks, "running")} · 已完成 ${countOf(tasks, "success")} · 失败 ${countOf(
        tasks,
        "failed"
    )}`;
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

/* ---------------- 空态（YBR4K） ---------------- */

/** 空态块高度：70vh 在 1200×800 基准下正好等于设计稿的 560；窗口变矮先收这里（下限 400 保证
 *  图标盒+两行文案+按钮 176 的内容不破版），变高封顶 640，免得拉成一整屏空白。 */
function EmptyTasks() {
    const { switchPrimary } = useNavigation();
    return (
        <div className="flex h-[clamp(400px,70vh,640px)] flex-col items-center justify-center gap-4 rounded-[12px] bg-bg-app px-5 py-10">
            <div className="flex flex-col items-center gap-4">
                <span className="flex size-14 items-center justify-center rounded-2xl bg-surface-2">
                    <Inbox className="size-6 text-text-3" />
                </span>
                <div className="flex flex-col items-center gap-1">
                    <span className="font-mono text-[16px] leading-[24px] font-semibold text-text-1">
                        还没有转换任务
                    </span>
                    <span className="font-mono text-[12px] leading-[18px] font-normal text-text-3">
                        上传 Minecraft 整合包后，转换任务会出现在这里
                    </span>
                </div>
            </div>
            <Btn
                variant="primary"
                className="border border-stroke text-[12px] font-medium"
                onClick={() => switchPrimary("home")}
            >
                去首页选择整合包
            </Btn>
        </div>
    );
}

/* ---------------- 卡片行：键盘删除与补位弹簧挂在这一层 ---------------- */

/**
 * motion.div 只做「这张卡在列表里的位置」，卡面仍是 TaskCard。
 * `layout="position"`：补位只平移不缩形（等高弹簧的等价物，正是样片里 FLIP 在做的事）。
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
            transition={{
                ...REFLOW,
                // 错峰按可见序号给；reduced-motion 下弹簧本来就被收掉，不必再排队
                delay: prefersReducedMotion() ? 0 : Math.min(index, 5) * STAGGER_STEP,
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
            <TaskCard task={task} onDelete={fly} />
        </motion.div>
    );
}

/* ---------------- 任务卡 ---------------- */

function TaskCard({ task, onDelete }: { task: ConversionTask; onDelete: () => void }) {
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
        if (res.queued) notify("已有转换正在进行，重试任务已加入队列", "info");
    };

    const cancel = async () => {
        await api.cancelTask(task.id);
        notify("任务已取消，已下载的文件保留在缓存", "info");
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
            notify(`打开输出目录失败：${e instanceof Error ? e.message : String(e)}`, "error");
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
                    <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        {subLine(task, outName)}
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
                        {task.error.title} · {task.error.detail}
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
                            删除
                        </Btn>
                    )}
                    {running && (
                        <Btn size="sm" onClick={() => void cancel()}>
                            取消
                        </Btn>
                    )}
                    {task.status === "success" && (
                        <Btn size="sm" onClick={() => void openOutput()}>
                            打开输出位置
                        </Btn>
                    )}
                    {(task.status === "failed" || task.status === "cancelled") && (
                        <Btn size="sm" icon={RefreshCw} onClick={() => void retry()}>
                            {task.status === "failed" ? "重试" : "重新转换"}
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
                        {PRIMARY_LABEL[task.status]}
                    </Btn>
                </div>
            </div>
        </Panel>
    );
}

/** row1 副标：输出名（运行/成功）或加载器 + 中断阶段（失败） */
function subLine(task: ConversionTask, outName: string): string {
    if (task.status === "failed") {
        return `→ ${loaderLabel(task.pack.loader)} ${task.options.mcVersion} · 中断于${stageLabel(
            task.stage ?? "builder",
            needsNetwork(task)
        )}阶段`;
    }
    if (task.status === "success" && task.outputSizeBytes != null) {
        return `→ ${outName} · ${formatSize(task.outputSizeBytes)}`;
    }
    return `→ ${outName}`;
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
        const last = task.logs[task.logs.length - 1]?.message;
        const where = task.stage ? `${stageLabel(task.stage, needsNetwork(task))} ·` : "排队 ·";
        return [where, last ?? "等待开始…"].filter(Boolean).join(" ");
    }
    if (task.status === "success") {
        const c = task.counts;
        return c ? `剔除 ${c.remove} 个客户端专属模组 · 补齐 ${c.add} 个服务端依赖` : "转换完成";
    }
    return "任务已取消 · 已取回的文件保留在下载缓存";
}

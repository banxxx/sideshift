/**
 * 任务列表页 Tasks（SS.pen `YUUJQ`，空态 `YBR4K`）
 *
 * 结构：页头（标题 + 等宽统计副标 + 右侧分段筛选）→ 任务卡列表（gap16）。
 * 单卡（gap12 padding20）三行：
 *  row1 = 36×36 状态图标盒 + 包名/输出名两行 + 状态芯片 + 右侧耗时
 *  row2 = 进度条 + 百分比（仅运行中）
 *  row3 = 左等宽明细行 + 右操作按钮组（失败态改为错误盒 + 右对齐按钮）
 * 空态为 560 高无边框块：56×56 图标盒 + 两行等宽文案 + accent 主按钮。
 */
import { Check, Download, Inbox, RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { useNavigation } from "@/lib/navigation";
import { BAR_COLOR, needsNetwork, progressChip, runCounts, stageLabel } from "@/lib/rail-view";
import { formatDuration, formatElapsed, formatSize, loaderLabel, outputNameOf, truncateMiddle } from "@/lib/format";
import type { ConversionTask, TaskStatus } from "@/lib/types";
import { Bar, Btn, PageHeader, Panel, SegTabs, ToneChip } from "@/components/design/ui";
import { cn } from "@/lib/utils";

type Filter = "all" | "running" | "success" | "failed";

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

export function TasksPage() {
    const [tasks, setTasks] = useState<ConversionTask[]>([]);
    const [filter, setFilter] = useState<Filter>("all");

    // 1s 轮询：mock 引擎推进与后端任务态切换都靠它刷新
    useEffect(() => {
        const load = () =>
            api
                .listTasks()
                .then((list) => setTasks([...list].sort((a, b) => b.createdAt - a.createdAt)))
                .catch(() => setTasks([]));
        void load();
        const timer = window.setInterval(load, 1000);
        return () => window.clearInterval(timer);
    }, []);

    const visible = tasks.filter((t) => matchesFilter(filter, t));

    return (
        <div className="flex flex-col gap-5">
            <PageHeader
                compact
                title="转换任务"
                sub={tasks.length === 0 ? "从首页选择整合包，开始第一次转换" : summary(tasks)}
                subTone="mono"
                right={<SegTabs items={FILTERS} value={filter} onChange={setFilter} />}
            />

            {tasks.length === 0 ? (
                <EmptyTasks />
            ) : (
                <div className="flex flex-col gap-4">
                    {visible.map((t) => (
                        <TaskCard key={t.id} task={t} />
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

/* ---------------- 任务卡 ---------------- */

function TaskCard({ task }: { task: ConversionTask }) {
    const { navigate } = useNavigation();
    const running = task.status === "running" || task.status === "queued";
    const icon = CARD_ICON[task.status];
    const Icon = icon.icon;
    const chip = progressChip(task);
    const started = task.startedAt ?? task.createdAt;
    const elapsed = (task.finishedAt ?? Date.now()) - started;
    const outName = task.outputFileName ?? outputNameOf(task.pack.fileName);

    const retry = async () => {
        const res = await api.retryTask(task.id);
        if (!res) return;
        if (res.queued) notify("已有转换正在进行，重试任务已加入队列", "info");
        navigate("task", { taskId: res.taskId });
    };

    const cancel = async () => {
        await api.cancelTask(task.id);
        notify("任务已取消，已下载的文件保留在缓存", "info");
    };

    /** 删除同样不许静默失败（列表 1s 轮询会收掉这行，所以成功时无需提示） */
    const del = async () => {
        try {
            await api.deleteTask(task.id);
        } catch (e) {
            notify(`删除任务失败：${e instanceof Error ? e.message : String(e)}`, "error");
        }
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
                    {/* 五态统一有详情出口：运行/排队/失败/已取消看日志，成功看报告 */}
                    {running ? (
                        <>
                            <Btn size="sm" onClick={() => void cancel()}>
                                取消
                            </Btn>
                            <Btn size="sm" onClick={() => navigate("task", { taskId: task.id })}>
                                查看日志
                            </Btn>
                        </>
                    ) : task.status === "success" ? (
                        <>
                            <Btn variant="danger" size="sm" onClick={() => void del()}>
                                删除
                            </Btn>
                            <Btn size="sm" onClick={() => void openOutput()}>
                                打开输出目录
                            </Btn>
                            <Btn
                                variant="primary"
                                size="sm"
                                className="font-semibold"
                                onClick={() => navigate("report", { taskId: task.id })}
                            >
                                查看报告
                            </Btn>
                        </>
                    ) : task.status === "failed" ? (
                        <>
                            <Btn variant="danger" size="sm" onClick={() => void del()}>
                                删除
                            </Btn>
                            <Btn size="sm" onClick={() => navigate("task", { taskId: task.id })}>
                                查看日志
                            </Btn>
                            <Btn variant="primary" size="sm" icon={RefreshCw} onClick={() => void retry()}>
                                重试
                            </Btn>
                        </>
                    ) : (
                        <>
                            <Btn variant="danger" size="sm" onClick={() => void del()}>
                                删除
                            </Btn>
                            <Btn size="sm" onClick={() => navigate("task", { taskId: task.id })}>
                                查看日志
                            </Btn>
                            <Btn variant="primary" size="sm" className="font-semibold" onClick={() => void retry()}>
                                重新转换
                            </Btn>
                        </>
                    )}
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

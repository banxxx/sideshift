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
import { useNavigation } from "@/lib/navigation";
import { BAR_COLOR, progressChip, stageLabel } from "@/lib/rail-view";
import { formatDuration, formatElapsed, formatSize, loaderLabel, outputNameOf } from "@/lib/format";
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

    const visible = tasks.filter((t) =>
        filter === "all"
            ? true
            : filter === "running"
              ? t.status === "running" || t.status === "queued"
              : t.status === filter
    );

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

function countOf(tasks: ConversionTask[], filter: Exclude<Filter, "all">): number {
    return tasks.filter((t) =>
        filter === "running" ? t.status === "running" || t.status === "queued" : t.status === filter
    ).length;
}

/* ---------------- 空态（YBR4K） ---------------- */

function EmptyTasks() {
    const { switchPrimary } = useNavigation();
    return (
        <div className="flex h-[560px] flex-col items-center justify-center gap-4 rounded-[12px] bg-bg-app px-5 py-10">
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
        const id = await api.retryTask(task.id);
        if (id) navigate("task", { taskId: id });
    };

    const openOutput = () => void api.resolveOutputPath(outName).then((p) => void api.openDir(api.dirOf(p)));

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
                        {task.pack.fileName}
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
                    <span className="font-mono text-[11px] leading-[16px] font-normal text-redstone">
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
                    {(running || task.status === "failed") && (
                        <Btn size="sm" onClick={() => navigate("task", { taskId: task.id })}>
                            查看日志
                        </Btn>
                    )}
                    {running && (
                        <Btn size="sm" onClick={() => void api.cancelTask(task.id)}>
                            取消
                        </Btn>
                    )}
                    {task.status === "failed" && (
                        <Btn variant="primary" size="sm" icon={RefreshCw} onClick={() => void retry()}>
                            重试
                        </Btn>
                    )}
                    {task.status === "success" && (
                        <>
                            <Btn variant="danger" size="sm" onClick={() => void api.deleteTask(task.id)}>
                                删除
                            </Btn>
                            <Btn size="sm" onClick={openOutput}>
                                打开输出目录
                            </Btn>
                        </>
                    )}
                    {task.status === "cancelled" && (
                        <>
                            <Btn variant="danger" size="sm" onClick={() => void api.deleteTask(task.id)}>
                                删除
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
            task.stage ?? "builder"
        )}阶段`;
    }
    if (task.status === "success" && task.outputSizeBytes != null) {
        return `→ ${outName} · ${formatSize(task.outputSizeBytes)}`;
    }
    return `→ ${outName}`;
}

/** 状态芯片文案：下载阶段追加 "7/12" 计数（设计稿 "下载中 7/12"） */
function chipLabel(task: ConversionTask, base: string): string {
    if (task.status === "running" && task.stage === "downloader" && task.downloaded != null && task.total != null) {
        return `${base} ${task.downloaded}/${task.total}`;
    }
    return base;
}

/** row3 左侧明细行 */
function detailLine(task: ConversionTask): string {
    if (task.status === "running" || task.status === "queued") {
        const last = task.logs[task.logs.length - 1]?.message;
        return [task.stage ? `${task.stage} ·` : "", last ?? "等待开始…"].filter(Boolean).join(" ");
    }
    if (task.status === "success") {
        const c = task.counts;
        return c ? `剔除 ${c.remove} 个客户端专属模组 · 补齐 ${c.add} 个服务端依赖` : "转换完成";
    }
    return "任务已取消 · 已下载文件保留在缓存";
}

/**
 * 任务详情页 Task（SS.pen Running `XjfIJ` / Failed `KdHjU` / Cancelled `ZKwyq`）
 *
 * 布局与 Convert 同构：BodyRow gap20 = 左列（gap16）+ 右栏 280 任务信息卡。
 * 左列两张卡：
 *  - 转换进度（gap12）：标题 + 状态芯片 → 进度条 h6 → 两行明细 → 分隔线 → 当前站/下一站微型轨道
 *  - 日志（gap12）：$surface-2 日志盒，行 = [stage] 等宽 10 紫 + 消息 等宽 10
 * 失败任务在左列顶部追加错误卡（Errors 族规范，提供重试/诊断出口）。
 *
 * 右栏按钮序（设计稿三态）：
 *  - running   查看转换方案 → 取消转换(红字) → 返回首页
 *  - failed    查看转换方案 → 重试转换(accent) → 返回首页
 *  - cancelled 查看转换方案 → 重新转换(accent) → 返回首页
 *  - success   查看转换报告(accent) → 返回首页
 */
import { useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import { useNavigation } from "@/lib/navigation";
import {
    BAR_COLOR,
    progressChip,
    stageLabel,
    stageTrack,
    toneDot,
    toneText,
} from "@/lib/rail-view";
import { formatClock, formatDuration, loaderLabel, outputNameOf, truncateMiddle } from "@/lib/format";
import type { ConversionTask } from "@/lib/types";
import { TaskErrorCard } from "@/components/features/TaskErrorCard";
import { Bar, Btn, Divider, InfoRow, PageHeader, Panel, PanelHead, ToneChip } from "@/components/design/ui";
import { cn } from "@/lib/utils";

/** HH:MM（设计稿时间口径，不含秒） */
const hm = (t: number) => formatClock(t).slice(0, 5);

export function TaskDetailPage() {
    const { entry, navigate, switchPrimary } = useNavigation();
    const taskId = entry.params?.taskId as string | undefined;

    const [task, setTask] = useState<ConversionTask | null>(null);
    const [missing, setMissing] = useState(false);
    const [copied, setCopied] = useState(false);
    const logRef = useRef<HTMLDivElement>(null);

    // 自调度轮询：运行/排队中每 800ms 拉一次快照，进入终态即停；taskId 变化（重试跳转）重新起表
    useEffect(() => {
        if (!taskId) return;
        let alive = true;
        let timer = 0;
        setTask(null);
        setMissing(false);

        const tick = async () => {
            const t = await api.getTask(taskId);
            if (!alive) return;
            if (!t) {
                setMissing(true);
                return;
            }
            setTask({ ...t });
            if (t.status === "running" || t.status === "queued") {
                timer = window.setTimeout(() => void tick(), 800);
            }
        };
        void tick();

        return () => {
            alive = false;
            window.clearTimeout(timer);
        };
    }, [taskId]);

    if (!taskId || missing) {
        return (
            <div className="flex flex-col gap-5">
                <PageHeader title="任务详情" />
                <Panel className="items-center py-16">
                    <p className="text-[13px] text-text-2">任务不存在或已过期。</p>
                    <Btn variant="primary" size="sm" className="mt-1" onClick={() => switchPrimary("tasks")}>
                        返回任务列表
                    </Btn>
                </Panel>
            </div>
        );
    }

    if (!task) {
        return (
            <div className="flex flex-col gap-5">
                <PageHeader title="任务详情" sub="正在读取任务状态…" />
                <Panel className="items-center py-16">
                    <span className="h-4 w-40 animate-pulse rounded bg-stroke" />
                </Panel>
            </div>
        );
    }

    const chip = progressChip(task);
    const track = stageTrack(task);
    const running = task.status === "running" || task.status === "queued";
    const started = task.startedAt ?? task.createdAt;
    const elapsed = (task.finishedAt ?? Date.now()) - started;
    const lastLog = task.logs[task.logs.length - 1];
    const counts = task.counts;
    const packName = truncateMiddle(
        task.pack.fileName.replace(/\.(mrpack|zip|7z)$/i, ""),
        32
    );

    const retry = async () => {
        const id = await api.retryTask(task.id);
        if (id) navigate("task", { taskId: id });
    };

    const copyDiagnostics = async () => {
        const err = task.error;
        if (!err) return;
        const text = [
            `[${err.stage}] ${err.title}`,
            err.detail,
            err.exitCode != null ? `exit code: ${err.exitCode}` : "",
            ...(err.logTail ?? []),
        ]
            .filter(Boolean)
            .join("\n");
        await navigator.clipboard.writeText(text);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
    };

    return (
        <div className="flex flex-col gap-5">
            <PageHeader title={packName} sub={subLine(task, elapsed)} />

            <div className="flex items-start gap-5">
                {/* 左列：错误卡（失败时）+ 转换进度 + 日志 */}
                <div className="flex min-w-0 flex-1 flex-col gap-4">
                    {task.error && (
                        <TaskErrorCard
                            error={task.error}
                            fileName={task.error.stage === "parser" ? task.pack.fileName : undefined}
                            onRetry={() => void retry()}
                            onFix={
                                task.error.stage === "parser"
                                    ? () => switchPrimary("home")
                                    : task.error.stage === "detector"
                                      ? () => navigate("convert", { manifest: task.pack })
                                      : undefined
                            }
                            onShowLog={
                                task.error.stage === "parser"
                                    ? () => logRef.current?.scrollIntoView({ behavior: "smooth", block: "end" })
                                    : undefined
                            }
                            onCopyDiagnostics={
                                task.error.stage === "builder" ? () => void copyDiagnostics() : undefined
                            }
                            copied={copied}
                        />
                    )}

                    {/* ---- 转换进度 ---- */}
                    <Panel gap={12}>
                        <PanelHead
                            inline
                            title="转换进度"
                            right={
                                <ToneChip
                                    tone={chip.tone}
                                    size="sm"
                                    className={cn("px-2.5", chip.plain && "bg-surface-2")}
                                >
                                    {chip.label}
                                </ToneChip>
                            }
                        />
                        <Bar percent={task.progress} fillClass={BAR_COLOR[task.status]} />

                        <div className="flex w-full justify-between gap-3">
                            <span className="text-[12px] leading-[18px] font-medium text-text-1">
                                {progressHeadline(task)}
                            </span>
                            <span className="shrink-0 font-mono text-[11px] leading-[16px] font-normal text-text-2">
                                {task.progress}%
                            </span>
                        </div>
                        <div className="flex w-full justify-between gap-3">
                            <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                                {lastLog?.message ?? "等待日志…"}
                            </span>
                            <span
                                className={cn(
                                    "shrink-0 font-mono text-[11px] leading-[16px] font-normal",
                                    task.status === "failed" ? "text-redstone" : "text-text-3"
                                )}
                            >
                                {progressAside(task, elapsed)}
                            </span>
                        </div>

                        <Divider />

                        {/* 微型轨道：当前站 → 下一站 */}
                        <div className="flex w-full items-center gap-2.5">
                            <span className={cn("size-2 shrink-0 rounded-full", toneDot(track.current.tone))} />
                            <span
                                className={cn(
                                    "text-[11px] leading-[16px] font-semibold",
                                    toneText(track.current.tone)
                                )}
                            >
                                {track.current.label}
                            </span>
                            {track.next && (
                                <>
                                    <span className="h-0.5 w-9 shrink-0 rounded-full bg-stroke" />
                                    <span className="size-2 shrink-0 rounded-full bg-stroke" />
                                    <span className="text-[11px] leading-[16px] font-normal text-text-3">
                                        {track.next}
                                    </span>
                                </>
                            )}
                        </div>
                    </Panel>

                    {/* ---- 日志 ---- */}
                    <Panel gap={12}>
                        <PanelHead title="日志" />
                        <div
                            ref={logRef}
                            className="flex max-h-[260px] w-full flex-col gap-1 overflow-auto rounded-lg bg-surface-2 p-3"
                        >
                            {task.logs.length === 0 && (
                                <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                    等待开始转换…
                                </span>
                            )}
                            {task.logs.map((l, i) => (
                                <div key={i} className="flex w-full gap-2">
                                    <span className="shrink-0 font-mono text-[10px] leading-[14px] font-normal text-amethyst">
                                        [{l.stage}]
                                    </span>
                                    <span
                                        className={cn(
                                            "min-w-0 flex-1 font-mono text-[10px] leading-[14px] font-normal",
                                            l.level === "error"
                                                ? "text-redstone"
                                                : l.level === "warn"
                                                  ? "text-text-3"
                                                  : "text-text-2"
                                        )}
                                    >
                                        {l.message}
                                    </span>
                                </div>
                            ))}
                        </div>
                    </Panel>
                </div>

                {/* 右栏：任务信息 */}
                <aside className="w-[280px] shrink-0">
                    <Panel gap={10}>
                        <PanelHead title="任务信息" />
                        <InfoRow label="开始时间" value={hm(started)} />
                        <InfoRow label="已用时长" value={formatDuration(elapsed)} />
                        <InfoRow
                            label="转换方案"
                            value={
                                counts
                                    ? `剔除 ${counts.remove} · 保留 ${counts.keep} · 新增 ${counts.add}`
                                    : "—"
                            }
                        />
                        <Divider />

                        <Btn size="sm" full className="text-text-1" onClick={() => navigate("convert", { manifest: task.pack })}>
                            查看转换方案
                        </Btn>
                        {running && (
                            <Btn
                                size="sm"
                                variant="danger"
                                full
                                onClick={() => void api.cancelTask(task.id)}
                            >
                                取消转换
                            </Btn>
                        )}
                        {task.status === "failed" && (
                            <Btn size="sm" variant="primary" full className="font-semibold" onClick={() => void retry()}>
                                重试转换
                            </Btn>
                        )}
                        {task.status === "cancelled" && (
                            <Btn size="sm" variant="primary" full className="font-semibold" onClick={() => void retry()}>
                                重新转换
                            </Btn>
                        )}
                        {task.status === "success" && (
                            <Btn
                                size="sm"
                                variant="primary"
                                full
                                className="font-semibold"
                                onClick={() => navigate("report", { taskId: task.id })}
                            >
                                查看转换报告
                            </Btn>
                        )}
                        <Btn size="sm" full onClick={() => switchPrimary("home")}>
                            返回首页
                        </Btn>

                        <p className="w-full text-center text-[10px] leading-[14px] font-normal text-text-3">
                            {task.status === "failed"
                                ? "已下载文件保留在缓存 · 可在设置中切换下载镜像源"
                                : task.status === "cancelled"
                                  ? "已下载文件保留在缓存 · 重新转换可续用"
                                  : "已下载文件保留在缓存，重试无需重新下载"}
                        </p>
                    </Panel>
                </aside>
            </div>
        </div>
    );
}

/* ---------------- 文案派生 ---------------- */

/** 页头副标：加载器 · 版本 · 状态 · 时间（三态措辞取自设计稿） */
function subLine(task: ConversionTask, elapsed: number): string {
    const base = `${loaderLabel(task.pack.loader)} · Minecraft ${task.options.mcVersion}`;
    switch (task.status) {
        case "running":
            return `${base} · 转换进行中 · ${hm(task.startedAt ?? task.createdAt)} 开始`;
        case "queued":
            return `${base} · 排队中`;
        case "failed":
            return `${base} · 转换失败 · ${task.finishedAt ? hm(task.finishedAt) : ""} 中断${
                task.error?.attempts ? `（已重试 ${task.error.attempts} 次）` : ""
            }`;
        case "cancelled":
            return `${base} · 已取消 · ${task.finishedAt ? hm(task.finishedAt) : ""}`;
        default:
            return `${base} · 转换成功 · 耗时 ${formatDuration(elapsed)}`;
    }
}

/** 进度卡第一行左侧：阶段级摘要 */
function progressHeadline(task: ConversionTask): string {
    const dl =
        task.downloaded != null && task.total != null
            ? `${task.downloaded} / ${task.total} 个文件`
            : "";
    if (task.status === "failed") return dl ? `依赖下载中断于 ${dl}` : `${stageLabel(task.stage ?? "builder")}阶段中断`;
    if (task.status === "cancelled") return dl ? `用户取消于 ${dl}` : "用户取消任务";
    if (task.status === "success") return `构建完成 · 输出 ${task.outputFileName ?? outputNameOf(task.pack.fileName)}`;
    if (task.stage === "downloader" && dl) return `依赖下载 ${dl}`;
    return `${stageLabel(task.stage ?? "parser")}阶段进行中`;
}

/** 进度卡第二行右侧：错误码 / 取消 / 耗时 */
function progressAside(task: ConversionTask, elapsed: number): string {
    if (task.status === "failed")
        return task.error?.attempts ? `已重试 ${task.error.attempts} 次` : "已中断";
    if (task.status === "cancelled") return "已取消";
    if (task.status === "success") return `耗时 ${formatDuration(elapsed)}`;
    return "";
}

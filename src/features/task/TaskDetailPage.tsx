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
import { notify } from "@/lib/notify";
import { useNavigation } from "@/lib/navigation";
import {
    BAR_COLOR,
    fetchCounts,
    needsNetwork,
    progressChip,
    stageLabel,
    stageTrack,
    toneDot,
    toneText,
} from "@/lib/rail-view";
import {
    formatClock,
    formatDuration,
    formatSize,
    formatStamp,
    loaderLabel,
    outputNameOf,
    truncateMiddle,
} from "@/lib/format";
import type { ActivityInfo, ConversionTask } from "@/lib/types";
import { TaskErrorCard } from "@/features/task/TaskErrorCard";
import { ActivitySubBar, activityMeasure } from "@/features/task/ActivityBar";
import { LogCopyButton } from "@/components/shared/LogCopyButton";
import { Bar, Btn, Divider, InfoRow, PageHeader, Panel, PanelHead, Tip, TIP_TRIGGER, ToneChip } from "@/components/ui";
import { useLogFollow } from "@/lib/log-view";
import { cn } from "@/lib/utils";

/** HH:MM（状态行短语用；字段级的时间一律 formatStamp 带年月日） */
const hm = (t: number) => formatClock(t).slice(0, 5);

/**
 * 日志渲染条数上限：后端已按窗口聚合并截断到 600 行，这里再钉一道渲染上限，
 * 保证 DOM 行数与日志量脱钩（一次转换曾产生 7300+ 行，全量渲染直接把窗口卡死）。
 * 复制按钮仍走全量 `task.logs`。
 */
const LOG_RENDER_CAP = 400;

export function TaskDetailPage() {
    const { entry, navigate, switchPrimary } = useNavigation();
    const taskId = entry.params?.taskId as string | undefined;

    const [task, setTask] = useState<ConversionTask | null>(null);
    const [missing, setMissing] = useState(false);
    const [copied, setCopied] = useState(false);
    /** 实时条数据源：进度事件比 800ms 轮询密一个量级，轮询到的快照作为兜底覆写 */
    const [activity, setActivity] = useState<ActivityInfo | undefined>(undefined);
    const logRef = useRef<HTMLDivElement>(null);
    useLogFollow(logRef, task?.logs.length ?? 0);

    // 事件流只喂实时条：日志/阶段等仍以轮询快照为准，避免两套状态互相覆写
    const eventsLive = useRef(false);
    useEffect(() => {
        let alive = true;
        let unsub: (() => void) | undefined;
        void api.onProgress((e) => {
            if (e.taskId !== taskId) return;
            eventsLive.current = true;
            setActivity(e.activity);
        }).then((fn) => {
            if (alive) unsub = fn;
            else fn();
        });
        return () => {
            alive = false;
            unsub?.();
        };
    }, [taskId]);

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
            // 拿不到事件流的环境（浏览器 mock / 订阅失败）用快照喂实时条
            if (!eventsLive.current) setActivity(t.activity);
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
    /** 当前动作只在运行中有意义：终态下事件里的残值不该再画子条 */
    const act = task.status === "running" ? activity : undefined;
    const hiddenLogs = Math.max(0, task.logs.length - LOG_RENDER_CAP);
    const visibleLogs = hiddenLogs > 0 ? task.logs.slice(-LOG_RENDER_CAP) : task.logs;
    const counts = task.counts;
    const packName = truncateMiddle(
        task.pack.fileName.replace(/\.(mrpack|zip|7z)$/i, ""),
        32
    );

    const retry = async () => {
        const res = await api.retryTask(task.id);
        if (!res) return;
        if (res.queued) notify("已有转换正在进行，重试任务已加入队列", "info");
        navigate("task", { taskId: res.taskId });
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
                                      ? () =>
                                            navigate("convert", {
                                                taskId: task.id,
                                                manifest: task.pack,
                                            })
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
                        {/* 父子两条一组：粗=整包总进度（阶段加权，分钟级），细=当前动作（秒级字节量） */}
                        <div className="flex w-full flex-col gap-1.5">
                            <Bar percent={task.progress} fillClass={BAR_COLOR[task.status]} />
                            <ActivitySubBar activity={act} />
                        </div>

                        <div className="flex w-full justify-between gap-3">
                            <span className="text-[12px] leading-[18px] font-medium text-text-1">
                                {progressHeadline(task)}
                            </span>
                            <span className="shrink-0 font-mono text-[11px] leading-[16px] font-normal text-text-2">
                                {task.progress}%
                            </span>
                        </div>
                        {/* 第二行：有当前动作时是「正在弄哪个文件」，否则回落最后一条日志。
                            完整文件名走 Tip（外层只管截断，气泡要在裁刀之外才不会被切掉） */}
                        <div className="flex w-full justify-between gap-3">
                            <span className={cn(TIP_TRIGGER, "flex min-w-0 flex-1 items-center")}>
                                <span
                                    className={cn(
                                        "min-w-0 truncate font-mono text-[11px] leading-[16px] font-normal",
                                        act ? "text-text-2" : "text-text-3"
                                    )}
                                >
                                    {act
                                        ? `${act.kind === "net" ? "下载" : "打包"} · ${act.subject}`
                                        : (lastLog?.message ?? "等待日志…")}
                                </span>
                                <Tip label={act?.subject ?? lastLog?.message} align="start" wide />
                            </span>
                            <span
                                className={cn(
                                    "shrink-0 font-mono text-[11px] leading-[16px] font-normal tabular-nums",
                                    act?.attempt && act.attempt > 1
                                        ? "text-gold"
                                        : task.status === "failed"
                                          ? "text-redstone"
                                          : "text-text-3"
                                )}
                            >
                                {act ? activityMeasure(act) : progressAside(task, elapsed)}
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

                    {/* ---- 日志：定高 260 + 框内滚动，卡高不随日志条数变化 ---- */}
                    <Panel gap={12}>
                        <PanelHead
                            title="日志"
                            right={
                                <LogCopyButton
                                    logs={task.logs}
                                    header={`SideShift 日志 · ${task.pack.fileName} · ${task.id}`}
                                />
                            }
                        />
                        <div
                            ref={logRef}
                            className="log-scroll flex h-[260px] w-full flex-col gap-1 overflow-y-auto rounded-lg bg-surface-2 p-3"
                        >
                            {task.logs.length === 0 && (
                                <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                    等待开始转换…
                                </span>
                            )}
                            {hiddenLogs > 0 && (
                                <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                    （仅显示最近 {LOG_RENDER_CAP} 条 · 已省略 {hiddenLogs} 条，复制可取全部{" "}
                                    {task.logs.length} 条）
                                </span>
                            )}
                            {visibleLogs.map((l, i) => (
                                <div key={i} className="flex w-full gap-2">
                                    <span className="shrink-0 font-mono text-[10px] leading-[14px] font-normal text-amethyst">
                                        [{l.stage}]
                                    </span>
                                    <span
                                        className={cn(
                                            "min-w-0 flex-1 break-words font-mono text-[10px] leading-[14px] font-normal",
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
                        <InfoRow label="开始时间" value={formatStamp(started)} />
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

                        <Btn
                            size="sm"
                            full
                            className="text-text-1"
                            onClick={() =>
                                navigate("convert", { taskId: task.id, manifest: task.pack })
                            }
                        >
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

/**
 * 进度卡第一行左侧：阶段级摘要。
 * 取件阶段按真实来源分流——真联网叫「下载 已联网/需联网」，
 * 包内/本地/缓存零流量的叫「取件 已完成/全部」并标注无需联网
 */
function progressHeadline(task: ConversionTask): string {
    const net = needsNetwork(task);
    const { verb, done, total } = fetchCounts(task);
    const n = task.total != null ? `${done} / ${total} 个文件` : "";
    const taken =
        task.doneBytes != null && task.doneBytes > 0 ? ` · 已取 ${formatSize(task.doneBytes)}` : "";
    if (task.status === "failed")
        return n ? `${verb}中断于 ${n}` : `${stageLabel(task.stage ?? "builder", net)}阶段中断`;
    if (task.status === "cancelled") return n ? `用户取消于 ${n}` : "用户取消任务";
    if (task.status === "success")
        return `构建完成 · 输出 ${task.outputFileName ?? outputNameOf(task.pack.fileName)}`;
    if (task.stage === "downloader" && n)
        return `${verb === "下载" ? "依赖" : "文件"}${verb} ${n}${taken}${net ? "" : " · 无需联网"}`;
    return `${stageLabel(task.stage ?? "parser", net)}阶段进行中`;
}

/** 进度卡第二行右侧：错误码 / 取消 / 耗时 */
function progressAside(task: ConversionTask, elapsed: number): string {
    if (task.status === "failed")
        return task.error?.attempts ? `已重试 ${task.error.attempts} 次` : "已中断";
    if (task.status === "cancelled") return "已取消";
    if (task.status === "success") return `耗时 ${formatDuration(elapsed)}`;
    return "";
}

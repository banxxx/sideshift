/**
 * 任务详情页 Task（SS.pen Running `XjfIJ` / Failed `KdHjU` / Cancelled `ZKwyq` / Success `Q45BPj`）
 *
 * 这里是任务列表唯一的下钻目的地：报告与方案都收进本页页签，不再各自占一条路由。
 * 页签集合按状态给（不适用的签不出现），签与签之间不设跳转按钮——换看别的内容只顶上的页签。
 *  - 概况：错误卡（失败时）+ 转换进度 + 日志
 *  - 结果：报告全文（仅已完成）
 *  - 方案：创建时确认过的方案快照，纯只读（全状态都给——它就是构建实际吃进去的那批决策，
 *    跑完不会作废，产物出问题时第一个要查的正是「当时剔了哪个模组」）
 *
 * 布局与 Convert 同构：BodyRow gap20 = 左列（gap16）+ 右栏 280 任务信息卡。
 * 左列概况页两张卡：
 *  - 转换进度（gap12）：标题 + 状态芯片 → 进度条 h6 → 两行明细 → 分隔线 → 当前站/下一站微型轨道
 *  - 日志（gap12）：$surface-2 日志盒，行 = [stage] 等宽 10 紫 + 消息 等宽 10
 *
 * 右栏是本页唯一的动作区，顺序固定为「主状态动作 → 次要动作 → 返回任务列表（永远最后一条）」：
 *  - running/queued  取消转换(红字) → 返回任务列表
 *  - failed          重试转换(accent) → 返回任务列表
 *  - cancelled       重新转换(accent) → 返回任务列表
 *  - success         打开输出位置(accent) → 复制转换方案 → 返回任务列表
 *
 * 顶部是**吸顶页头**（与任务列表页同一套规则）：这一页能滚出两屏以上，页签一旦够不着，
 * 页面就只剩当前那一签。换签是一次横向翻页（`TAB_SWEEP`：点右侧那档就从右边进来），
 * 与页签上滑动的选中胶囊同一条轴；退场层走 popLayout 抽离文档流，容器不会先塌一次高度。
 */
import { AnimatePresence, motion } from "motion/react";
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
import type { ActivityInfo, ConversionReport, ConversionTask } from "@/lib/types";
import { TaskErrorCard } from "@/features/task/TaskErrorCard";
import { ActivitySubBar, activityMeasure } from "@/features/task/ActivityBar";
import { LogCopyButton } from "@/components/shared/LogCopyButton";
import { PlanReviewView } from "@/features/convert/PlanReviewView";
import { buildPlanSummary, ReportView } from "@/features/report/ReportView";
import {
    Bar,
    Btn,
    Divider,
    InfoRow,
    PageHeader,
    Panel,
    PanelHead,
    SegTabs,
    Tip,
    TIP_TRIGGER,
    ToneChip,
} from "@/components/ui";
import { useLogFollow } from "@/lib/log-view";
import { CARD_RISE, PAGE_RISE, TAB_SWEEP } from "@/lib/page-motion";
import { cn } from "@/lib/utils";

/** HH:MM（状态行短语用；字段级的时间一律 formatStamp 带年月日） */
const hm = (t: number) => formatClock(t).slice(0, 5);

/**
 * 日志渲染条数上限：后端已按窗口聚合并截断到 600 行，这里再钉一道渲染上限，
 * 保证 DOM 行数与日志量脱钩（一次转换曾产生 7300+ 行，全量渲染直接把窗口卡死）。
 * 复制按钮仍走全量 `task.logs`。
 */
const LOG_RENDER_CAP = 400;

/**
 * 页签 key：概况与方案人人有份——方案是创建时确认、构建时真吃进去的那批决策（`get_task_plan`
 * 读存档快照），跑完不会作废，所以「跑完了就不用看打算」这个直觉是错的：产物出问题，
 * 第一个要查的就是当时剔了哪个模组。只有「结果」（报告）是完成后才存在的东西。
 */
type TaskTab = "overview" | "result" | "plan";

const TAB_LABEL: Record<TaskTab, string> = {
    overview: "概况",
    result: "结果",
    plan: "方案",
};

/** 状态 → 可见页签集合；集合外的 key 一律回落概况，列表带进来的落点不会和状态打架 */
function tabsOf(status: ConversionTask["status"]): TaskTab[] {
    return status === "success"
        ? ["overview", "result", "plan"]
        : ["overview", "plan"];
}

export function TaskDetailPage() {
    const { entry, switchPrimary } = useNavigation();
    const taskId = entry.params?.taskId as string | undefined;
    /** 列表主按钮带着落点来（已完成→结果，其余→概况） */
    const wanted = entry.params?.tab as TaskTab | undefined;

    const [task, setTask] = useState<ConversionTask | null>(null);
    const [missing, setMissing] = useState(false);
    const [copied, setCopied] = useState(false);
    const [planCopied, setPlanCopied] = useState(false);
    const [tab, setTab] = useState<TaskTab>(wanted ?? "overview");
    /** 重试起表轮询用：同一 id 原地重跑，不该把同一个页面再压一层栈 */
    const [nonce, setNonce] = useState(0);
    /** 报告与产物路径：右栏动作（打开输出位置/复制方案）与结果签共用，只在这一处装载 */
    const [report, setReport] = useState<ConversionReport | null>(null);
    const [outPath, setOutPath] = useState<string | null>(null);
    /** 实时条数据源：进度事件比 800ms 轮询密一个量级，轮询到的快照作为兜底覆写 */
    const [activity, setActivity] = useState<ActivityInfo | undefined>(undefined);
    const logRef = useRef<HTMLDivElement>(null);
    /** 换页签的方向（±1）：交给 TAB_SWEEP 决定内容进出的那一侧。ref 而不是 state——
     *  它只是给动画读的旁证，改它不该单独触发一次渲染。 */
    const tabDir = useRef(0);
    useLogFollow(logRef, task?.logs.length ?? 0);

    const succeeded = task?.status === "success";

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

    // 换任务先清屏，免得新 id 的骨架期还画着上一条的内容；重试（nonce）不清，避免整页闪一下
    useEffect(() => {
        setTask(null);
        setMissing(false);
        setReport(null);
        setOutPath(null);
    }, [taskId]);

    // 自调度轮询：运行/排队中每 800ms 拉一次快照，进入终态即停；taskId 变化或重试重新起表
    useEffect(() => {
        if (!taskId) return;
        let alive = true;
        let timer = 0;

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
    }, [taskId, nonce]);

    // 报告只在成功后才有；产物路径优先用后端回传的真实 outputPath，旧记录缺字段才按输出目录重建
    useEffect(() => {
        if (!taskId || !succeeded || !task) return;
        let alive = true;
        void (async () => {
            const r = await api.getReport(taskId);
            if (!alive) return;
            setReport(r ?? null);
            const name = r?.outputFileName ?? task.outputFileName;
            const full =
                task.outputPath ??
                (name
                    ? await api.resolveOutputPath(
                          name,
                          task.options.outputOverride?.trim() || undefined
                      )
                    : undefined);
            if (alive) setOutPath(full ?? null);
        })();
        return () => {
            alive = false;
        };
    }, [taskId, succeeded, task]);

    if (!taskId || missing) {
        return (
            <div className="flex flex-col gap-5 py-6">
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
            <div className="flex flex-col gap-5 py-6">
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

    // 只有「结果」是完成后才有的签：原地重试让状态退回运行中时，当前签就不在集合里了，
    // 渲染期直接回落概况（不用 effect 同步，也就不会有一帧挂着已消失的签）
    const items = tabsOf(task.status);
    const active: TaskTab = items.includes(tab) ? tab : "overview";

    /** 页签一律走这里换：顺手把方向记下来（点在右侧那一档 = 新内容从右边进来） */
    const changeTab = (next: TaskTab) => {
        tabDir.current = items.indexOf(next) >= items.indexOf(active) ? 1 : -1;
        setTab(next);
    };

    /** 打开产物所在目录：静默失败会被当成「按钮坏了」，一律把错误外显到全局提示区 */
    const openOutput = async () => {
        if (!outPath) {
            notify("这条记录没有产物路径信息，无法定位输出目录", "warn");
            return;
        }
        try {
            await api.openDir(api.dirOf(outPath));
        } catch (e) {
            notify(`打开输出目录失败：${e instanceof Error ? e.message : String(e)}`, "error");
        }
    };

    const copyPlan = async () => {
        if (!report) return;
        try {
            await navigator.clipboard.writeText(buildPlanSummary(task, report, outPath));
            setPlanCopied(true);
            window.setTimeout(() => setPlanCopied(false), 2000);
        } catch (e) {
            notify(`复制方案失败：${e instanceof Error ? e.message : String(e)}`, "error");
        }
    };

    const retry = async () => {
        const res = await api.retryTask(task.id);
        if (!res) return;
        if (res.queued) notify("已有转换正在进行，重试任务已加入队列", "info");
        // 同一 id 原地重跑：回到概况、清掉上一轮的报告，重新起轮询而不是再压一层导航栈
        changeTab("overview");
        setReport(null);
        setOutPath(null);
        setNonce((n) => n + 1);
    };

    const cancel = async () => {
        await api.cancelTask(task.id);
        notify("任务已取消，已下载的文件保留在缓存", "info");
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
        <motion.div
            className="flex flex-col gap-5 pb-6"
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
        >
            {/* 吸顶页头：概况签在窄窗口下能滚出两屏，方案/结果更是长清单——页签够不着就等于
                这一页只剩当前那一签。盒子与规则同任务列表页：留白这圈由吸顶盒自己出（pt-6），
                pb-5 + -mb-5 抵掉根 div 的 gap-5（静止观感逐像素不变，但那 20px 归底色管），
                z-20 压过带 transform 的卡片层。 */}
            <motion.div
                variants={CARD_RISE}
                className="sticky top-0 z-20 -mb-5 bg-background pt-6 pb-5"
            >
                <PageHeader
                    title={packName}
                    sub={subLine(task, elapsed)}
                    right={
                        <SegTabs
                            items={items.map((k) => ({ key: k, label: TAB_LABEL[k] }))}
                            value={active}
                            onChange={changeTab}
                        />
                    }
                />
            </motion.div>

            <motion.div variants={CARD_RISE} className="flex items-start gap-5">
                {/* 左列按页签换内容：这里不摆跨页按钮，换看别的内容只顶上那排页签。
                    换页是一次横向翻页（TAB_SWEEP：点哪一侧就从哪一侧进来），
                    popLayout 让退场那层抽离文档流，容器高度当场就是新内容的，不会先塌一次。 */}
                <div className="relative flex min-w-0 flex-1 flex-col gap-4">
                    <AnimatePresence mode="popLayout" initial={false} custom={tabDir.current}>
                        <motion.div
                            key={active}
                            custom={tabDir.current}
                            variants={TAB_SWEEP}
                            initial="hidden"
                            animate="show"
                            exit="exit"
                            className="flex flex-col gap-4"
                        >
                            {active === "overview" && (
                                <>
                                    {task.error && (
                                        <TaskErrorCard
                                            error={task.error}
                                            fileName={
                                                task.error.stage === "parser"
                                                    ? task.pack.fileName
                                                    : undefined
                                            }
                                            onRetry={() => void retry()}
                                            onFix={
                                                task.error.stage === "parser"
                                                    ? () => switchPrimary("home")
                                                    : task.error.stage === "detector"
                                                      ? () => changeTab("plan")
                                                      : undefined
                                            }
                                            onShowLog={
                                                task.error.stage === "parser"
                                                    ? () =>
                                                          logRef.current?.scrollIntoView({
                                                              behavior: "smooth",
                                                              block: "end",
                                                          })
                                                    : undefined
                                            }
                                            onCopyDiagnostics={
                                                task.error.stage === "builder"
                                                    ? () => void copyDiagnostics()
                                                    : undefined
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
                                            <Bar
                                                percent={task.progress}
                                                fillClass={BAR_COLOR[task.status]}
                                            />
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
                                            <span
                                                className={cn(
                                                    TIP_TRIGGER,
                                                    "flex min-w-0 flex-1 items-center"
                                                )}
                                            >
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
                                                <Tip
                                                    label={act?.subject ?? lastLog?.message}
                                                    align="start"
                                                    wide
                                                />
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
                                                {act
                                                    ? activityMeasure(act)
                                                    : progressAside(task, elapsed)}
                                            </span>
                                        </div>

                                        <Divider />

                                        {/* 微型轨道：当前站 → 下一站 */}
                                        <div className="flex w-full items-center gap-2.5">
                                            <span
                                                className={cn(
                                                    "size-2 shrink-0 rounded-full",
                                                    toneDot(track.current.tone)
                                                )}
                                            />
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
                                            className="log-scroll flex h-[260px] w-full flex-col gap-1 overflow-y-auto rounded-md bg-surface-2 p-3"
                                        >
                                            {task.logs.length === 0 && (
                                                <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                                    等待开始转换…
                                                </span>
                                            )}
                                            {hiddenLogs > 0 && (
                                                <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                                    （仅显示最近 {LOG_RENDER_CAP} 条 · 已省略 {hiddenLogs}{" "}
                                                    条，复制可取全部 {task.logs.length} 条）
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
                                </>
                            )}

                            {active === "result" &&
                                (report ? (
                                    <ReportView
                                        taskId={task.id}
                                        report={report}
                                        task={task}
                                        outPath={outPath}
                                    />
                                ) : (
                                    <Panel className="items-center py-16">
                                        <span className="h-4 w-40 animate-pulse rounded bg-stroke" />
                                    </Panel>
                                ))}

                            {active === "plan" && (
                                <PlanReviewView taskId={task.id} manifest={task.pack} />
                            )}
                        </motion.div>
                    </AnimatePresence>
                </div>

                {/* 右栏：任务信息 + 本页唯一的动作区 */}
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

                        {running && (
                            <Btn
                                size="sm"
                                variant="danger"
                                full
                                className="font-medium"
                                onClick={() => void cancel()}
                            >
                                取消转换
                            </Btn>
                        )}
                        {task.status === "failed" && (
                            <Btn
                                size="sm"
                                variant="primary"
                                full
                                className="font-semibold"
                                onClick={() => void retry()}
                            >
                                重试转换
                            </Btn>
                        )}
                        {task.status === "cancelled" && (
                            <Btn
                                size="sm"
                                variant="primary"
                                full
                                className="font-semibold"
                                onClick={() => void retry()}
                            >
                                重新转换
                            </Btn>
                        )}
                        {task.status === "success" && (
                            <>
                                <Btn
                                    variant="primary"
                                    full
                                    className="font-semibold"
                                    onClick={() => void openOutput()}
                                >
                                    打开输出位置
                                </Btn>
                                <Btn
                                    size="sm"
                                    full
                                    className="bg-surface text-text-1"
                                    onClick={() => void copyPlan()}
                                >
                                    {planCopied ? "已复制方案" : "复制转换方案"}
                                </Btn>
                            </>
                        )}
                        {/* 回退固定占动作组最后一条 */}
                        <Btn size="sm" full onClick={() => switchPrimary("tasks")}>
                            返回任务列表
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
            </motion.div>
        </motion.div>
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

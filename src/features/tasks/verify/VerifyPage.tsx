/**
 * 本机开服验证页：任务详情「本地开服验证」的下一层。一副骨架走九档状态（结论卡 + 六步清单 + 控制台 + 右栏参数与动作）。
 * 验的是交付包解出来的副本，交付包全程只读；一轮只留最新结果，副本与日志要用户点「清理验证目录」才删。
 * 数据只有 `get_verify_state` 这一份快照：进度事件按 250ms 合批回拉，运行中另起 800ms 轮询兜底（浏览器 mock 没有事件流）。
 */
import {
    Ban,
    CircleCheck,
    CircleX,
    Clock,
    FileQuestion,
    FolderOpen,
    Info,
    Play,
    RefreshCw,
    Settings2,
    ShieldAlert,
    TriangleAlert,
    type LucideIcon,
} from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { useNavigation } from "@/lib/navigation";
import { errOf } from "@/lib/errors";
import { msgText } from "@/lib/log-normalize";
import { formatDuration, formatSize, loaderLabel, truncateMiddle } from "@/lib/format";
import { t, tSource, useT } from "@/lib/i18n";
import type {
    BackendMsg,
    ConversionTask,
    StepStatus,
    VerifyLogLine,
    VerifySession,
    VerifySource,
    VerifyStatus,
    VerifyStep,
    VerifyStepId,
} from "@/lib/types";
import type { Tone } from "@/components/ui";
import {
    Bar,
    Btn,
    Collapse,
    Divider,
    InfoRow,
    MiniMeta,
    NoteRow,
    Panel,
    PanelHead,
    PageHeader,
    TagChip,
    Tip,
    TIP_TRIGGER,
    ToneChip,
} from "@/components/ui";
import { LogCopyButton } from "@/components/shared/LogCopyButton";
import { useLogFollow } from "@/lib/log-view";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { cn } from "@/lib/utils";
import { ModToggleModal } from "@/features/tasks/verify/ModToggleModal";

/** 控制台定高：卡高不随日志条数变化（稿量 280） */
const CONSOLE_H = 280;

/**
 * 渲染条数上限：后端日志环已经裁到 600，这里再钉一道，保证 DOM 行数与日志量脱钩。
 * 复制走的仍是全量 `session.logs`。
 */
const RENDER_CAP = 400;

/** 副本里的控制台落盘名（Rust: `task_engine::verify::LOG_FILE`），「打开日志文件」定位的就是它 */
const LOG_FILE = "server.log";

/**
 * 这一页会露出的后端句子（`core::verify_run` 与 `task_engine::verify` 的 `msg!`）：三个入口都在本页——
 * 控制台原文、六步摘要、阻塞项与失败提示。登记过才翻得动，漏一条英文界面就露简体。
 * 逐字与 Rust 侧对齐，改动必须两头一起（`node scripts/i18n.mjs sync` 重生成锁）。
 */
/*i18n:
    backend.vf-pack-gone=交付包不在了：{{path}}
    backend.vf-no-output-path=这条任务没有记下交付包路径，先把它转换完成
    backend.jdk-picked=方案里指定的那枚 Java 已不在本机，改用自动挑到的那一枚
    backend.vf-java-used=本次使用 Java {{have}}：{{path}}
    backend.vf-java-too-low=本机 Java {{have}} 低于本包需要的 Java {{need}}
    backend.vf-java-absent=本机没有检测到 Java，本包需要 Java {{need}} 及以上
    backend.vf-java-none=本机没有检测到可用的 Java
    backend.vf-eula-ok=交付包里 eula.txt 已同意最终用户协议
    backend.vf-eula-no=交付包里的 eula.txt 没有同意最终用户协议，服务端会拒绝启动（验证不代填这一项）
    backend.vf-eula-missing=交付包里没有 eula.txt，服务端会拒绝启动
    backend.vf-blocked=本机条件不满足，本次结果不可判定
    backend.vf-env-summary=Java {{have}} · 交付包 {{bytes}} 字节
    backend.vf-copy-reused=沿用上次副本：{{files}} 个文件 · {{bytes}} 字节
    backend.vf-copy-unpacked=已解出 {{files}} 个文件 · {{bytes}} 字节
    backend.vf-mod-counts=mods 目录共 {{total}} 枚模组，其中 {{disabled}} 枚处于停用状态
    backend.vf-no-launch-cmd=副本里没有以 java 开头的启动命令（start 与 run 脚本都没有）
    backend.vf-launch-cmd=按 {{script}} 起进程：{{command}}
    backend.vf-spawn-fail=起进程失败：{{cause}}
    backend.vf-process-up=进程已起 · {{script}}
    backend.vf-ready=服务端已就绪（{{sec}} 秒）
    backend.vf-stop-forced=写了 stop 也没能自己退，进程已被终止（不影响通过判定，退出码记为空）
    backend.vf-exited-early=进程退出（{{code}}），没有等到就绪
    backend.vf-timeout={{secs}} 秒内没有等到就绪，进程已终止（不判包坏）
    backend.vf-cancelled=已取消：服务端进程已终止，本次不作判定
    backend.vf-no-state=读不到进程状态，本次不作判定
    backend.vf-no-selfcheck=构建时没有开启自检，这一步没有依据
    backend.vf-selfcheck-fail=构建自检有 {{failed}} 项未通过（共 {{total}} 项）
    backend.vf-selfcheck-warn=构建自检 {{passed}} 项通过、{{warned}} 项提示
    backend.vf-selfcheck-pass=构建自检 {{passed}} 项全部通过
    backend.vf-loader-fabric=Fabric 的加载器随服务端首启自装，这一步不单独跑安装器
    backend.vf-loader-built=构建时已把加载器并进产物，副本里就是装好的那份
    backend.vf-loader-nojar=副本根目录里没有加载器安装器 jar，首启自装也无从执行
    backend.vf-loader-installed=已在副本里装出 {{files}} 个文件 · {{bytes}} 字节
    backend.vf-port-switched=端口 {{from}} 已被本机其它程序占用，本次改用空闲端口 {{to}}：新值只写进副本的 server.properties，交付包不动
    backend.vf-port-writefail=端口 {{from}} 已被占用，空闲端口 {{to}} 写不回副本，仍用原端口起进程：{{cause}}
    backend.vf-interrupted=应用已关闭，上一轮验证中断，本次不作判定
    backend.vf-cancel-requested=已请求取消，服务端进程会被终止
    backend.vf-mod-enabled=已启用 {{file}}，重新验证后生效
    backend.vf-mod-disabled=已停用 {{file}}，重新验证后生效
*/

/**
 * 「尚未开始」那一档后端不给步骤清单（会话是默认档，`steps` 为空），这六条用来先把清单摆出来。
 * 词与 `core::verify_run::STEP_DEFS` 逐字一致，并且和真步骤走同一个 `tSource` 查表口径。
 */
/*i18n:
    verify.step-label-env=检查本机环境
    verify.step-label-unpack=解包交付包
    verify.step-label-reconcile=产物对账
    verify.param-loader=首启装加载器
    verify.step-label-launch=启动服务端进程
    verify.step-label-wait=等待 Done
*/
const PENDING_STEPS: [VerifyStepId, string][] = [
    ["env", "检查本机环境"],
    ["unpack", "解包交付包"],
    ["reconcile", "产物对账"],
    ["loader", "首启装加载器"],
    ["launch", "启动服务端进程"],
    ["wait", "等待 Done"],
];

type Variant = "primary" | "outline" | "ghost" | "danger";
type Meta = { label: string; value: string };
type PrimaryKey = "start" | "cancel" | "settings" | "output";
type ActionKey = "open-copy" | "open-mods" | "clean" | "output" | "recheck";

/** 建表在函数里、每格一条 `t(字面量)`：顶层建表会把文案冻在首次加载的语言上 */
function stepWord(status: StepStatus): { label: string; tone: Tone } {
    const table: Record<StepStatus, { label: string; tone: Tone }> = {
        pending: { label: t("verify.step-pending", "未开始"), tone: "muted" },
        running: { label: t("verify.step-running", "进行中"), tone: "gold" },
        pass: { label: t("verify.step-pass", "通过"), tone: "emerald" },
        warn: { label: t("verify.step-warn", "提示"), tone: "gold" },
        fail: { label: t("verify.step-fail", "失败"), tone: "redstone" },
        skipped: { label: t("verify.step-skipped", "跳过"), tone: "muted" },
    };
    return table[status];
}

function statusTone(status: VerifyStatus): Tone {
    const table: Record<VerifyStatus, Tone> = {
        ready: "muted",
        running: "gold",
        passed: "emerald",
        failed: "redstone",
        timeout: "gold",
        cancelled: "muted",
        blocked: "amethyst",
        missing: "redstone",
    };
    return table[status];
}

function statusWord(status: VerifyStatus): string {
    const table: Record<VerifyStatus, string> = {
        ready: t("verify.chip-ready", "未验证"),
        running: t("verify.chip-running", "验证中"),
        passed: t("verify.chip-passed", "通过"),
        failed: t("verify.chip-failed", "失败"),
        timeout: t("verify.chip-timeout", "超时"),
        cancelled: t("lib.canceled", "已取消"),
        blocked: t("verify.chip-blocked", "不可判定"),
        missing: t("verify.chip-missing", "交付包不存在"),
    };
    return table[status];
}

/** 日志行的来源标签色：环境 / 副本 / 加载器与 JVM / 服务端本体 / Mixin 各一档，眼睛能顺着一条腿读下去 */
function sourceClass(src: VerifySource): string {
    const table: Record<VerifySource, string> = {
        env: "text-diamond",
        unpack: "text-amethyst",
        verify: "text-amethyst",
        loader: "text-gold",
        java: "text-gold",
        server: "text-text-3",
        mixin: "text-redstone",
    };
    return table[src];
}

/** 分钟数只有这一处用（等待上限 1800 秒该读成「30 分钟」而不是「30分 0秒」） */
const minutes = (sec: number) => Math.max(1, Math.round(sec / 60));

/** 会话里走得到的那几步；默认档没有清单时退回那六个名字（全 pending） */
function stepsOf(s: VerifySession): VerifyStep[] {
    return s.steps.length > 0
        ? s.steps
        : PENDING_STEPS.map(([id, label]) => ({
              id,
              label,
              status: "pending" as StepStatus,
              note: "",
              noteMsg: null,
          }));
}

/** 断在哪一步：失败档认 `failure.step`，其余档只能给到最后走到的那一格。返回后端原词，翻由调用处做 */
function failingLabel(s: VerifySession, steps: VerifyStep[]): string {
    const id = s.failure?.step;
    const hit = id ? steps.find((x) => x.id === id) : undefined;
    return hit?.label ?? "";
}

export function VerifyPage() {
    const t = useT();
    const { entry, back, switchPrimary } = useNavigation();
    const taskId = entry.params?.taskId as string | undefined;

    const [task, setTask] = useState<ConversionTask | null>(() =>
        taskId ? api.peekTask(taskId) ?? null : null
    );
    const [taskMissing, setTaskMissing] = useState(false);
    /** null = 快照还没落地（≠ 读到了默认档）：首帧铺默认档会先闪一屏「未验证」 */
    const [session, setSession] = useState<VerifySession | null>(null);
    const [outPath, setOutPath] = useState<string | null>(null);
    const [modsOpen, setModsOpen] = useState(false);
    /** 开始与清理之后重新起表：轮询那条链在终档是停着的，不重开就看不到新一轮 */
    const [nonce, setNonce] = useState(0);
    const logRef = useRef<HTMLDivElement>(null);
    useLogFollow(logRef);

    useEffect(() => {
        if (!taskId) return;
        let alive = true;
        void api
            .getTask(taskId)
            .then((tk) => {
                if (!alive) return;
                if (!tk) setTaskMissing(true);
                else setTask({ ...tk });
            })
            .catch(() => {});
        return () => {
            alive = false;
        };
    }, [taskId, nonce]);

    // 快照轮询：只有 running 续表，其余档拉到一帧就停
    useEffect(() => {
        if (!taskId) return;
        let alive = true;
        let timer = 0;
        const tick = async () => {
            const snap = await api.getVerifyState(taskId).catch(() => null);
            if (!alive || !snap) return;
            setSession(snap);
            if (snap.status === "running") timer = window.setTimeout(() => void tick(), 800);
        };
        void tick();
        return () => {
            alive = false;
            window.clearTimeout(timer);
        };
    }, [taskId, nonce]);

    /* 进度事件每行日志一条（服务端首启能刷屏），按 250ms 合批回拉一次快照：
       界面只认快照那一份真相，事件在这里只当「该重拉了」的门铃。 */
    useEffect(() => {
        if (!taskId) return;
        let alive = true;
        let due = 0;
        let seq = 0;
        const pull = () => {
            const mine = ++seq;
            void api
                .getVerifyState(taskId)
                .then((snap) => {
                    // 只认最后发出的那次：两次请求的完成顺序不保证
                    if (alive && mine === seq) setSession(snap);
                })
                .catch(() => {});
        };
        const unsubs: Array<() => void> = [];
        const hold = (fn: () => void) => (alive ? unsubs.push(fn) : fn());
        void api
            .onVerifyProgress((e) => {
                if (e.taskId !== taskId || due) return;
                due = window.setTimeout(() => {
                    due = 0;
                    pull();
                }, 250);
            })
            .then(hold)
            .catch(() => {});
        // 一轮跑完 / 目录清掉：立刻拉，不等合批
        void api
            .onVerifyDone((id) => {
                if (id !== taskId) return;
                window.clearTimeout(due);
                due = 0;
                pull();
            })
            .then(hold)
            .catch(() => {});
        return () => {
            alive = false;
            window.clearTimeout(due);
            unsubs.forEach((fn) => fn());
        };
    }, [taskId]);

    // 交付包路径：任务记下 outputPath 就用它，旧记录缺字段才按输出目录重建（与任务详情同一口径）
    useEffect(() => {
        if (!task) return;
        let alive = true;
        void (async () => {
            const name = task.outputFileName;
            const full =
                task.outputPath ??
                (name
                    ? await api.resolveOutputPath(name, task.options.outputOverride?.trim() || undefined)
                    : undefined);
            if (alive) setOutPath(full ?? null);
        })();
        return () => {
            alive = false;
        };
    }, [task]);

    if (!taskId || taskMissing) {
        return (
            <div className="flex flex-col gap-5 py-6">
                <PageHeader title={t("verify.title", "本地开服验证")} />
                <Panel className="items-center py-16">
                    <p className="text-[13px] text-text-2">
                        {t("tasks.task-found", "任务不存在或已过期。")}
                    </p>
                    <Btn
                        variant="primary"
                        size="sm"
                        className="mt-1"
                        onClick={() => switchPrimary("tasks")}
                    >
                        {t("tasks.back-tasks", "返回任务列表")}
                    </Btn>
                </Panel>
            </div>
        );
    }

    /* 快照落地前那一帧什么都不画。`useEffect` 排在浏览器 paint 之后，占位块必然被画出来一次；
       快照到了又把根节点从 div 换成 motion.div ⇒ 整棵树在入场动画演到一半时重挂，读出来就是「闪一下」。
       空帧由外层 `motion.main` 那 120ms 淡入盖掉，页面第一次落笔就是带数据的那一版。 */
    if (!task || !session) return null;

    const s = session;
    const steps = stepsOf(s);
    /** 走完的步数：未开始与进行中不算，跳过与提示算 */
    const doneSteps = steps.filter((x) => x.status !== "pending" && x.status !== "running").length;
    const current = steps.find((x) => x.status === "running") ?? steps[steps.length - 1];
    const hero = heroOf(s, task, steps, current, outPath);
    /** 控制台原文不降噪（后端同口径：这里要看的就是进程自己喊的话），只把「模板 + 参数」翻成当前档 */
    const rendered = s.logs.map((l) => lineText(l));
    const hiddenLines = Math.max(0, s.logs.length - RENDER_CAP);

    /** 一切「打不开就说话」的调用共用这一层：静默失败会被读成「按钮坏了」 */
    const guard = (fn: () => Promise<void>) => () => {
        void fn().catch((e) => notify(errOf(e), "error"));
    };
    const start = guard(async () => {
        await api.startVerify(task.id);
        setNonce((n) => n + 1);
    });
    const cancel = guard(async () => {
        const stopped = await api.cancelVerify(task.id);
        if (!stopped) notify(t("verify.nothing-cancel", "这一轮已经结束，没有可取消的进程"), "warn");
        setNonce((n) => n + 1);
    });
    const clean = guard(async () => {
        await api.cleanVerifyDir(task.id);
        notify(t("verify.copy-cleaned", "验证目录已清理，结论与日志一起删除"), "info");
        setNonce((n) => n + 1);
    });
    const openCopy = guard(async () => {
        if (!s.copyDir) {
            notify(t("verify.no-copy-yet", "还没有验证副本"), "warn");
            return;
        }
        await api.openDir(s.copyDir);
    });
    const openLog = guard(async () => {
        if (!s.copyDir) {
            notify(t("verify.no-copy-yet", "还没有验证副本"), "warn");
            return;
        }
        await api.revealPath(api.joinPath(s.copyDir, LOG_FILE));
    });
    const openOutput = guard(async () => {
        if (!outPath) {
            notify(t("tasks.output-path", "这条记录没有产物路径信息，无法定位输出目录"), "warn");
            return;
        }
        await api.openDir(api.dirOf(outPath));
    });

    /** 启停一枚模组之后：计数与那行「已停用…重新验证后生效」都从快照来，页面不自己加减 */
    const refresh = () => {
        void api
            .getVerifyState(task.id)
            .then(setSession)
            .catch(() => {});
    };

    const runners: Record<PrimaryKey | ActionKey, () => void> = {        start,
        cancel,
        settings: () => switchPrimary("settings"),
        output: openOutput,
        "open-copy": openCopy,
        "open-mods": () => setModsOpen(true),
        clean,
        recheck: start,
    };

    const primary = primaryOf(s);
    const actions = actionsOf(s);
    /** 卡本体与「这一档到底有没有卡」用同一个来源，别在 Collapse 的开关上重写一遍状态判据 */
    const card = stateCardOf(s, tSource(failingLabel(s, steps)));

    return (
        <motion.div
            className="flex flex-col gap-5 pb-6"
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
        >
            <div className="page-head-veil sticky top-0 z-20 pt-6 [--veil-gap:1.25rem]">
                <motion.div variants={CARD_RISE}>
                    <PageHeader
                        title={truncateMiddle(task.pack.fileName.replace(/\.(mrpack|zip|7z)$/i, ""), 32)}
                        sub={
                            `${loaderLabel(task.pack.loader)} · Minecraft ${task.options.mcVersion} · ` +
                            t("verify.from-task", "来自任务 {{id}} · 验证对象为解包副本", { id: task.id })
                        }
                        subTone="mono"
                        right={
                            <ToneChip size="sm" className="px-2.5" tone={statusTone(s.status)}>
                                {statusWord(s.status)}
                            </ToneChip>
                        }
                    />
                </motion.div>
            </div>

            {/* ---- 结论卡：状态 + 数字 + 唯一那颗主动作 + 控制台（整幅宽，排在两列之前） ---- */}
            <motion.div variants={CARD_RISE}>
                <Panel gap={12}>
                    <div className="flex w-full items-start gap-2.5">
                        <hero.Icon
                            className={cn("mt-px size-[18px] shrink-0", TONE_ICON[statusTone(s.status)])}
                        />
                        <div className="flex min-w-0 flex-col gap-0.5">
                            <span className="text-[16px] leading-[22px] font-semibold text-text-1">
                                {hero.title}
                            </span>
                            <span className="text-[13px] leading-[18px] font-normal text-text-2">
                                {hero.sub}
                            </span>
                        </div>
                    </div>

                    <div className="flex w-full flex-wrap gap-x-6 gap-y-3">
                        {hero.metas.map((m) => (
                            <MiniMeta key={m.label} label={m.label} value={m.value} />
                        ))}
                    </div>

                    {hero.bar && (
                        <div className="flex w-full flex-col gap-1.5">
                            <Bar
                                percent={hero.bar.percent}
                                fillClass={hero.bar.fill}
                                flow={s.status === "running"}
                            />
                            <div className="flex w-full justify-between gap-3">
                                <span className="text-[11px] leading-[16px] font-normal text-text-3">
                                    {t("verify.read-lines", "已读取 {{count}} 行日志", { count: s.logLines })}
                                </span>
                                <span className="shrink-0 font-mono text-[11px] leading-[16px] font-normal tabular-nums text-text-2">
                                    {t("verify.step-progress", "{{done}} / {{total}} 步", {
                                        done: doneSteps,
                                        total: steps.length,
                                    })}
                                </span>
                            </div>
                        </div>
                    )}

                    <Btn
                        variant={primary.variant}
                        size="sm"
                        full
                        className="font-semibold"
                        icon={primary.icon}
                        onClick={runners[primary.key]}
                    >
                        {primary.label}
                    </Btn>

                    <Divider />

                    <PanelHead
                        title={t("verify.console", "服务端控制台")}
                        right={
                            <div className="flex items-center gap-2">
                                {s.logLines > s.logs.length && (
                                    <ToneChip size="xs" tone="muted">
                                        {t("verify.ring-only", "日志环保留最近 {{count}} 条", {
                                            count: s.logs.length,
                                        })}
                                    </ToneChip>
                                )}
                                <Btn size="xs" variant="ghost" onClick={openLog}>
                                    {t("verify.open-log-file", "打开日志文件")}
                                </Btn>
                                <LogCopyButton
                                    logs={rendered.map((l) => ({ time: l.time, stage: l.source, message: l.text }))}
                                    header={`SideShift 验证日志 · ${task.pack.fileName} · ${task.id}`}
                                />
                            </div>
                        }
                    />

                    <div
                        ref={logRef}
                        style={{ height: CONSOLE_H }}
                        className="log-scroll flex w-full flex-col gap-1 overflow-y-auto rounded-sm bg-surface-2 p-3"
                    >
                        {hiddenLines > 0 && (
                            <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                {t(
                                    "verify.showing-last",
                                    "（仅显示最近 {{cap}} 条 · 已省略 {{hidden}} 条，复制可取全部 {{total}} 条）",
                                    { cap: RENDER_CAP, hidden: hiddenLines, total: s.logs.length }
                                )}
                            </span>
                        )}
                        {s.logs.length === 0 && (
                            <span className="font-mono text-[10px] leading-[14px] text-text-3">
                                {t("verify.no-log-yet", "还没有输出。")}
                            </span>
                        )}
                        {rendered.slice(-RENDER_CAP).map((l, i) => (
                            <div key={i} className="flex w-full gap-2.5">
                                <span className="shrink-0 font-mono text-[10px] leading-[14px] font-normal text-amethyst">
                                    [{l.time}]
                                </span>
                                <span
                                    className={cn(
                                        "shrink-0 font-mono text-[10px] leading-[14px] font-normal",
                                        sourceClass(l.source)
                                    )}
                                >
                                    [{l.source}]
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
                                    {l.text}
                                </span>
                            </div>
                        ))}
                    </div>

                    {/* 副本在哪：路径掐中间（文件名才是答案），完整串走 Tip */}
                    <div className="flex w-full items-center justify-between gap-3">
                        <span className={cn(TIP_TRIGGER, "flex min-w-0 flex-1 items-center")}>
                            <span className="min-w-0 truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                                {s.copyDir
                                    ? truncateMiddle(s.copyDir, 58)
                                    : t("verify.copy-none", "尚未创建验证副本")}
                            </span>
                            <Tip label={s.copyDir || undefined} align="start" wide />
                        </span>
                        <ToneChip size="xs" tone={s.copyDir ? "emerald" : "muted"}>
                            {s.copyDir
                                ? t("verify.copy-kept", "整份保留")
                                : t("verify.copy-none-chip", "未创建")}
                        </ToneChip>
                    </div>
                </Panel>
            </motion.div>

            <motion.div variants={CARD_RISE} className="flex items-start gap-5">
                {/* ---- 左列：六步清单 + 这一档专属的那张卡 ---- */}
                <div className="flex min-w-0 flex-1 flex-col gap-4">
                    <Panel gap={12}>
                        <PanelHead
                            title={t("verify.process", "执行过程")}
                            right={
                                <ToneChip size="xs" tone="muted">
                                    {t("verify.step-count", "{{count}} 步", { count: steps.length })}
                                </ToneChip>
                            }
                        />
                        <div className="w-full divide-y divide-stroke-soft">
                            {steps.map((x) => {
                                const w = stepWord(x.status);
                                const note = msgText(x.note, x.noteMsg);
                                return (
                                    <div
                                        key={x.id}
                                        className="flex w-full items-start justify-between gap-3 py-2"
                                    >
                                        <div className="flex min-w-0 flex-col gap-0.5">
                                            <span className="text-[12px] leading-[18px] font-medium text-text-1">
                                                {tSource(x.label)}
                                            </span>
                                            {note !== "" && (
                                                <span className="break-words text-[11px] leading-[16px] font-normal text-text-3">
                                                    {note}
                                                </span>
                                            )}
                                        </div>
                                        <ToneChip size="xs" tone={w.tone}>
                                            {w.label}
                                        </ToneChip>
                                    </div>
                                );
                            })}
                        </div>
                    </Panel>

                    {/* 整块挂卸 ⇒ Collapse：这一列是 flex flex-col gap-4，gap 传 16 */}
                    <Collapse when={card !== null} gap={16}>
                        {card}
                    </Collapse>
                </div>

                {/* ---- 右栏：参数 + 本页唯一动作区（宽见 `--aside-w`） ---- */}
                <aside className="w-[var(--aside-w)] shrink-0">
                    <Panel gap={10}>
                        <PanelHead title={t("verify.params", "验证参数")} />
                        {paramsOf(s, task).map((p) => (
                            <InfoRow key={p.label} label={p.label} value={p.value} />
                        ))}
                        <NoteRow icon={Info}>
                            {t("verify.params-note", "内存与正版验证取自副本里的启动脚本与 server.properties。")}
                        </NoteRow>

                        <Divider />

                        <PanelHead title={t("verify.actions", "操作")} />
                        {actions.map((a) => (
                            <Btn
                                key={a.key}
                                size="sm"
                                variant={a.variant}
                                full
                                disabled={a.disabled}
                                title={a.tip}
                                onClick={runners[a.key]}
                            >
                                {a.label}
                            </Btn>
                        ))}
                        <Btn size="sm" full onClick={back}>
                            {t("verify.back-detail", "返回任务详情")}
                        </Btn>

                        <Divider />

                        <InfoRow
                            label={t("verify.copy-size", "验证目录占用")}
                            value={s.copyBytes > 0 ? formatSize(s.copyBytes) : t("verify.copy-none-chip", "未创建")}
                        />
                        <p className="w-full text-center text-[10px] leading-[14px] font-normal text-text-3">
                            {footNoteOf(s)}
                        </p>
                    </Panel>
                </aside>
            </motion.div>

            <ModToggleModal
                open={modsOpen}
                onClose={() => setModsOpen(false)}
                taskId={task.id}
                onToggled={refresh}
            />
        </motion.div>
    );
}

/* ---------------- 状态文案与派生 ---------------- */

/** 结论卡图标色：跟状态芯片同一套 tone，别再单开一列颜色 */
const TONE_ICON: Record<Tone, string> = {
    emerald: "text-emerald",
    gold: "text-gold",
    redstone: "text-redstone",
    accent: "text-accent",
    amethyst: "text-amethyst",
    diamond: "text-diamond",
    muted: "text-text-3",
};

/** 一行控制台的取词：后端模板优先，外部原文照原句（这里不按形状降噪） */
function lineText(l: VerifyLogLine): { time: string; source: VerifySource; level: VerifyLogLine["level"]; text: string } {
    return { time: l.time, source: l.source, level: l.level, text: msgText(l.message, l.messageMsg) };
}

function heroOf(
    s: VerifySession,
    task: ConversionTask,
    steps: VerifyStep[],
    current: VerifyStep,
    outPath: string | null
): { Icon: LucideIcon; title: string; sub: string; metas: Meta[]; bar: { percent: number; fill: string } | null } {
    const elapsed = formatDuration(s.elapsedSec * 1000);
    const lines = t("verify.count-lines", "{{count}} 行", { count: s.logLines });
    const code = s.exitCode == null ? "—" : String(s.exitCode);
    const modCount =
        s.modTotal > 0
            ? t("verify.count-mods", "{{total}} 枚 · 停用 {{disabled}}", {
                  total: s.modTotal,
                  disabled: s.modDisabled,
              })
            : "—";
    const waitCap = t("verify.count-minutes", "{{count}} 分钟", { count: minutes(s.timeoutSec) });
    const copySize = s.copyBytes > 0 ? formatSize(s.copyBytes) : t("verify.copy-none-chip", "未创建");
    const failedAt = tSource(failingLabel(s, steps));
    const stage = tSource(current.label);
    const doneSteps = steps.filter((x) => x.status !== "pending" && x.status !== "running").length;
    /** 只有走进程的那三档画条：其余档的「完成度」是编出来的数 */
    const bar =
        s.status === "running" || s.status === "timeout" || s.status === "cancelled"
            ? {
                  percent: Math.round((doneSteps / steps.length) * 100),
                  fill:
                      s.status === "running" || s.status === "timeout"
                          ? "bg-bar-sand shadow-[var(--shadow-bar-sand)]"
                          : "bg-stroke",
              }
            : null;

    switch (s.status) {
        case "ready":
            return {
                Icon: Play,
                title: t("verify.hero-ready", "尚未开始验证"),
                sub: t("verify.hero-ready-sub", "开始后在解包副本上启动本地服务端，交付包只读。"),
                metas: [
                    { label: t("report.output-file", "交付包"), value: task.outputFileName ?? "—" },
                    {
                        label: t("verify.meta-plan", "转换方案"),
                        value: task.counts
                            ? t("tasks.remove-removed", "剔除 {{remove}} · 保留 {{keep}} · 新增 {{add}}", {
                                  remove: task.counts.remove,
                                  keep: task.counts.keep,
                                  add: task.counts.add,
                              })
                            : "—",
                    },
                    { label: t("verify.meta-wait", "就绪等待上限"), value: waitCap },
                    { label: t("verify.meta-copy", "验证副本"), value: t("verify.copy-none-chip", "未创建") },
                ],
                bar: null,
            };
        case "running":
            return {
                Icon: RefreshCw,
                title: t("verify.hero-running", "正在启动服务端"),
                sub: t("verify.hero-running-sub", "第 {{index}} / {{total}} 步：{{step}}。", {
                    index: steps.findIndex((x) => x.id === current.id) + 1,
                    total: steps.length,
                    step: stage,
                }),
                metas: [
                    { label: t("verify.meta-stage", "当前阶段"), value: stage },
                    { label: t("verify.meta-elapsed", "已运行"), value: elapsed },
                    { label: t("verify.meta-read", "已读取"), value: lines },
                    { label: t("verify.meta-copy-size", "副本大小"), value: copySize },
                ],
                bar,
            };
        case "passed":
            return {
                Icon: CircleCheck,
                title:
                    s.warnings && s.warnings.length > 0
                        ? t("verify.hero-passed-warn", "通过 · {{count}} 条警告", {
                              count: s.warnings.length,
                          })
                        : t("verify.hero-passed", "验证通过 · 服务端正常启动"),
                sub: t("verify.hero-passed-sub", "等到 Done 之后由验证写入 stop 让进程自己收尾，副本与日志整份保留。"),
                metas: [
                    { label: t("report.duration-used", "用时"), value: elapsed },
                    { label: t("verify.meta-exit", "退出码"), value: code },
                    { label: t("verify.meta-lines", "日志行数"), value: lines },
                    { label: t("verify.meta-mods", "副本模组"), value: modCount },
                ],
                bar: null,
            };
        case "failed":
            return {
                Icon: CircleX,
                title: t("verify.hero-failed", "服务端没能就绪"),
                sub: t("verify.hero-failed-sub", "进程在「{{step}}」退出，退出码 {{code}}。", {
                    step: failedAt,
                    code,
                }),
                metas: [
                    { label: t("verify.meta-fail-stage", "失败阶段"), value: failedAt },
                    { label: t("verify.meta-exit", "退出码"), value: code },
                    { label: t("report.duration-used", "用时"), value: elapsed },
                    { label: t("verify.meta-lines", "日志行数"), value: lines },
                ],
                bar: null,
            };
        case "timeout":
            return {
                Icon: Clock,
                title: t("verify.hero-timeout", "{{time}} 内没有等到就绪", { time: waitCap }),
                sub: t("verify.hero-timeout-sub", "进程已被终止。这一档说的是没等到那句就绪，不判交付包坏。"),
                metas: [
                    { label: t("verify.meta-stopped", "停在阶段"), value: stage },
                    { label: t("verify.meta-wait", "就绪等待上限"), value: waitCap },
                    { label: t("verify.meta-read", "已读取"), value: lines },
                    { label: t("verify.meta-copy-size", "副本大小"), value: copySize },
                ],
                bar,
            };
        case "cancelled":
            return {
                Icon: Ban,
                title: t("verify.hero-cancelled", "本次验证已取消"),
                sub: t("verify.hero-cancelled-sub", "服务端进程已终止，本次不作判定。"),
                metas: [
                    { label: t("verify.meta-stopped", "停在阶段"), value: stage },
                    { label: t("verify.meta-elapsed", "已运行"), value: elapsed },
                    { label: t("verify.meta-lines", "日志行数"), value: lines },
                    { label: t("verify.meta-process", "进程状态"), value: t("verify.process-killed", "已终止") },
                ],
                bar,
            };
        case "blocked":
            return {
                Icon: ShieldAlert,
                title: t("verify.hero-blocked", "本机条件不满足 · 本次不可判定"),
                sub: t("verify.hero-blocked-sub", "没有起进程，验证副本未创建。"),
                metas: [
                    {
                        label: t("verify.meta-blockers", "阻塞项"),
                        value: t("verify.count-items", "{{count}} 项", { count: s.blockers?.length ?? 0 }),
                    },
                    { label: t("verify.meta-stuck", "卡在哪"), value: stage },
                    { label: t("verify.meta-elapsed", "已运行"), value: elapsed },
                    { label: t("verify.meta-mods", "副本模组"), value: modCount },
                ],
                bar: null,
            };
        case "missing":
            return {
                Icon: FileQuestion,
                title: t("verify.hero-missing", "交付包不存在"),
                sub: t("verify.hero-missing-sub", "验证读的是输出目录里那份交付包：先重新转换，或把它放回原位。"),
                metas: [
                    { label: t("report.output-file", "交付包"), value: task.outputFileName ?? "—" },
                    {
                        label: t("verify.meta-looked", "查找路径"),
                        value: outPath ? truncateMiddle(outPath, 40) : "—",
                    },
                    { label: t("verify.meta-lines", "日志行数"), value: lines },
                    { label: t("verify.meta-copy", "验证副本"), value: t("verify.copy-none-chip", "未创建") },
                ],
                bar: null,
            };
    }
}

/** 结论卡那一颗主动作：一档只有一个出口，其余动作全在右栏 */
function primaryOf(s: VerifySession): {
    key: PrimaryKey;
    label: string;
    variant: Variant;
    icon: LucideIcon;
} {
    switch (s.status) {
        case "running":
            return {
                key: "cancel",
                label: t("verify.act-cancel", "取消验证"),
                variant: "danger",
                icon: Ban,
            };
        case "blocked":
            return {
                key: "settings",
                label: t("verify.act-settings", "去设置补齐环境"),
                variant: "primary",
                icon: Settings2,
            };
        case "missing":
            return {
                key: "output",
                label: t("verify.act-open-output", "打开输出目录"),
                variant: "primary",
                icon: FolderOpen,
            };
        case "ready":
            return {
                key: "start",
                label: t("verify.act-start", "开始验证"),
                variant: "primary",
                icon: Play,
            };
        default:
            return {
                key: "start",
                label: t("verify.act-again", "重新验证"),
                variant: "primary",
                icon: RefreshCw,
            };
    }
}

/** 右栏动作：每一档只给此刻做得成的那几颗 */
function actionsOf(s: VerifySession): {
    key: ActionKey;
    label: string;
    variant: Variant;
    disabled?: boolean;
    tip?: string;
}[] {
    const hasCopy = !!s.copyDir;
    const busy = s.status === "running";
    const openCopy = {
        key: "open-copy" as ActionKey,
        label: t("verify.act-open-copy", "打开验证目录"),
        variant: "outline" as Variant,
        disabled: !hasCopy,
        tip: hasCopy ? t("verify.act-open-copy-tip", "在资源管理器里打开这一份副本") : t("verify.no-copy-yet", "还没有验证副本"),
    };
    const openMods = {
        key: "open-mods" as ActionKey,
        label: t("verify.act-mods", "打开模组清单"),
        variant: "outline" as Variant,
        disabled: !hasCopy,
        tip: hasCopy ? undefined : t("verify.no-copy-yet", "还没有验证副本"),
    };
    const clean = {
        key: "clean" as ActionKey,
        label: t("verify.act-clean", "清理验证目录"),
        variant: "danger" as Variant,
        disabled: !hasCopy,
        tip: hasCopy
            ? t("verify.clean-tip", "副本整份删除，结论与日志一起没了")
            : t("verify.no-copy-yet", "还没有验证副本"),
    };

    switch (s.status) {
        case "ready":
            return [
                {
                    key: "output",
                    label: t("verify.act-open-output-2", "打开交付包目录"),
                    variant: "outline",
                },
            ];
        case "running":
            return [openCopy, { ...openMods, tip: busy ? t("verify.busy-tip", "验证进行中，改名要等这一轮结束") : undefined }];
        case "passed":
        case "failed":
            return [openCopy, openMods, clean];
        case "timeout":
        case "cancelled":
            return [openCopy, clean];
        case "blocked":
            return [
                { key: "recheck", label: t("verify.act-recheck", "重新检查环境"), variant: "outline" },
            ];
        case "missing":
            return [
                { key: "output", label: t("verify.act-open-output-2", "打开交付包目录"), variant: "outline" },
            ];
    }
}

/** 「本次验证」尾部那一行：说的是副本的去留，与裁决一致——除了清理，没有别的删除路径 */
function footNoteOf(s: VerifySession): string {
    switch (s.status) {
        case "ready":
            return t("verify.foot-ready", "验证只读交付包，改动只落在副本。");
        case "running":
            return t("verify.foot-running", "进行中 · 验证目录整份保留。");
        case "passed":
            return t("verify.foot-passed", "已通过 · 验证目录整份保留，清理请自己点。");
        case "failed":
        case "timeout":
            return t("verify.foot-kept", "验证目录整份保留，日志与副本都在原地。");
        case "cancelled":
            return t("verify.foot-cancelled", "已取消 · 验证目录仍保留。");
        case "blocked":
            return t("verify.foot-blocked", "本次未创建验证目录。");
        case "missing":
            return t("verify.foot-missing", "未创建验证目录。");
    }
}

/** 「验证参数」：读不到的项一律写「未取到」，不猜、也不按开关演 */
function paramsOf(s: VerifySession, task: ConversionTask): Meta[] {
    const missing = t("verify.not-read", "未取到");
    const o = task.options;
    const loader =
        task.pack.loader === "fabric"
            ? t("verify.loader-fabric", "随服务端首启")
            : s.loaderInstalled === null || s.loaderInstalled === undefined
              ? missing
              : s.loaderInstalled
                ? t("verify.loader-built", "构建时已装")
                : t("verify.loader-first", "需要 · 副本内首启");
    return [
        {
            label: t("home.loader", "加载器"),
            value: `${loaderLabel(task.pack.loader)} ${o.loaderVersion}`,
        },
        { label: t("report.game-version", "游戏版本"), value: `Minecraft ${o.mcVersion}` },
        { label: "Java", value: `${t("convert.java-required", "需要")} ${o.javaVersion}` },
        { label: t("verify.param-xmx", "JVM 内存"), value: s.jvmXmx ?? missing },
        {
            label: t("report.verify-accounts", "正版验证"),
            value:
                s.onlineMode == null
                    ? missing
                    : s.onlineMode
                      ? t("report.entry-2", "开启")
                      : t("report.off-offline", "关闭（离线）"),
        },
        {
            label: t("convert.server-port", "服务器端口"),
            value: String(s.serverPort ?? o.serverPort),
        },
        {
            label: t("verify.meta-wait", "就绪等待上限"),
            value: t("verify.wait-cap-minutes", "{{count}} 分钟 · 内置", {
                count: minutes(s.timeoutSec),
            }),
        },
        { label: t("verify.param-loader", "首启装加载器"), value: loader },
        {
            label: t("verify.param-mods", "副本模组"),
            value:
                s.modTotal > 0
                    ? t("verify.count-mods", "{{total}} 枚 · 停用 {{disabled}}", {
                          total: s.modTotal,
                          disabled: s.modDisabled,
                      })
                    : t("verify.copy-none-chip", "未创建"),
        },
    ];
}

/* ---------------- 这一档专属的那张卡 ---------------- */

/** 状态卡按档分发；没有卡的档位返回 null，调用处用它同时决定「有没有卡」和「卡长什么样」 */
function stateCardOf(s: VerifySession, failedAt: string) {
    switch (s.status) {
        case "failed":
            return <FailureCard s={s} failedAt={failedAt} />;
        case "timeout":
            return <TimeoutCard s={s} />;
        case "cancelled":
            return <CancelledCard />;
        case "blocked":
            return <BlockerCard items={s.blockers ?? []} />;
        case "missing":
            return <RecoverCard />;
        case "passed":
            return s.warnings && s.warnings.length > 0 ? <WarningsCard s={s} /> : null;
        default:
            return null;
    }
}

/**
 * 失败定位：后端只给证据（报错行点到的 jar、最近的报错原文、环境提示），归因由人做。
 * 行号是本次输出流的序号，不是 `server.log` 的行号——日志环裁过，两者对不上。
 */
function FailureCard({ s, failedAt }: { s: VerifySession; failedAt: string }) {
    const t = useT();
    const f = s.failure;
    const jars = f?.jars ?? [];
    const lines = f?.lines ?? [];
    const hints = (s.hints ?? []).map((m) => msgText(m.zh, m));
    return (
        <Panel gap={12}>
            <PanelHead
                title={t("verify.card-failure", "失败定位")}
                right={
                    <ToneChip size="xs" tone="redstone">
                        {t("verify.card-stopped-at", "断在「{{step}}」", { step: failedAt })}
                    </ToneChip>
                }
            />
            {jars.length > 0 && (
                <div className="flex w-full flex-wrap items-center gap-1.5">
                    <span className="text-[11px] leading-[16px] font-normal text-text-3">
                        {t("verify.jars-named", "报错行点到的 jar")}
                    </span>
                    {jars.map((j) => (
                        <TagChip key={j} square tone="gold">
                            {j}
                        </TagChip>
                    ))}
                </div>
            )}
            {lines.length > 0 && (
                <div className="flex w-full flex-col gap-1">
                    <span className="text-[11px] leading-[16px] font-normal text-text-3">
                        {t("verify.error-lines", "最近的报错原文 · 共 {{count}} 行", { count: s.logLines })}
                    </span>
                    <div className="log-scroll flex max-h-[152px] w-full flex-col gap-1 overflow-y-auto rounded-sm bg-surface-2 p-3">
                        {lines.map((l) => (
                            <div key={l.line} className="flex w-full gap-2.5">
                                <span className="shrink-0 font-mono text-[10px] leading-[14px] font-normal text-amethyst tabular-nums">
                                    #{l.line}
                                </span>
                                <span className="min-w-0 flex-1 break-words font-mono text-[10px] leading-[14px] font-normal text-redstone">
                                    {l.text}
                                </span>
                            </div>
                        ))}
                    </div>
                </div>
            )}
            {hints.map((h) => (
                <NoteRow key={h} icon={TriangleAlert} tone="gold">
                    {h}
                </NoteRow>
            ))}
            <span className="text-[11px] leading-[16px] font-normal text-text-3">
                {t(
                    "verify.common-causes",
                    "其他常见原因：模组与加载器版本不匹配、JVM 内存偏小、副本里的配置或存档损坏。"
                )}
            </span>
        </Panel>
    );
}

function TimeoutCard({ s }: { s: VerifySession }) {
    const t = useT();
    return (
        <Panel gap={12}>
            <PanelHead
                title={t("verify.card-timeout", "超时排查")}
                right={
                    <ToneChip size="xs" tone="gold">
                        {t("verify.card-not-broken", "不判包坏")}
                    </ToneChip>
                }
            />
            <ol className="flex w-full flex-col gap-1.5 pl-4">
                {[
                    t("verify.ts-1", "首启是否在联网下载依赖：断网或源慢会一直停在准备阶段。"),
                    t("verify.ts-2", "副本大小与内存上限：老机器上大包首启本来就会过这条线。"),
                    t("verify.ts-3", "是否首次生成世界：世界越大， preparing spawn area 那一段越久。"),
                ].map((x, i) => (
                    <li
                        key={i}
                        className="text-[11px] leading-[16px] font-normal text-text-2 marker:font-mono marker:text-text-3"
                    >
                        {x}
                    </li>
                ))}
            </ol>
            <span className="text-[11px] leading-[16px] font-normal text-text-3">
                {t(
                    "verify.timeout-read",
                    "本次等待 {{time}}，读到 {{count}} 行。副本停在最后一步的状态，打开目录对照日志看。",
                    {
                        time: t("verify.count-minutes", "{{count}} 分钟", { count: minutes(s.timeoutSec) }),
                        count: s.logLines,
                    }
                )}
            </span>
        </Panel>
    );
}

function WarningsCard({ s }: { s: VerifySession }) {
    const t = useT();
    const warnings = s.warnings ?? [];
    return (
        <Panel gap={12}>
            <PanelHead
                title={t("verify.card-warnings", "警告")}
                right={
                    <ToneChip size="xs" tone="gold">
                        {t("verify.count-items", "{{count}} 项", { count: warnings.length })}
                    </ToneChip>
                }
            />
            <div className="log-scroll flex max-h-[152px] w-full flex-col gap-1 overflow-y-auto rounded-sm bg-surface-2 p-3">
                {warnings.map((w) => (
                    <div key={w.line} className="flex w-full gap-2.5">
                        <span className="shrink-0 font-mono text-[10px] leading-[14px] font-normal text-amethyst tabular-nums">
                            #{w.line}
                        </span>
                        <span className="min-w-0 flex-1 break-words font-mono text-[10px] leading-[14px] font-normal text-text-2">
                            {w.text}
                        </span>
                    </div>
                ))}
            </div>
            <span className="text-[11px] leading-[16px] font-normal text-text-3">
                {t("verify.warn-source", "服务端自己喊出的 WARN 原文，行号是本次输出流的序号。")}
            </span>
        </Panel>
    );
}

/** 取消之后：只报这一档的真实去留，结论不产生、副本与日志仍整份留着 */
function CancelledCard() {
    const t = useT();
    return (
        <Panel gap={12}>
            <PanelHead title={t("verify.card-cancelled", "取消之后")} />
            <InfoRow label={t("verify.cc-verdict", "本次结论")} value={t("verify.cc-none", "不产生")} />
            <InfoRow
                label={t("verify.cc-copy", "验证副本")}
                value={t("verify.cc-kept", "整份保留")}
                valueClass="text-gold"
            />
            <InfoRow
                label={t("verify.cc-log", "日志")}
                value={t("verify.cc-kept-log", "留在副本与验证会话里")}
            />
            <span className="text-[11px] leading-[16px] font-normal text-text-3">
                {t("verify.cc-note", "要重来一次直接点「重新验证」；不要这份副本了，点「清理验证目录」。")}
            </span>
        </Panel>
    );
}

/** 阻塞项：后端那句「本机条件不满足」的两条腿（没装 / 低于需求线）都从这里摊 */
function BlockerCard({ items }: { items: BackendMsg[] }) {
    const t = useT();
    return (
        <Panel gap={12}>
            <PanelHead
                title={t("verify.card-blockers", "不可判定的原因")}
                right={
                    <ToneChip size="xs" tone="amethyst">
                        {t("verify.card-no-process", "未起进程")}
                    </ToneChip>
                }
            />
            {items.map((m, i) => (
                <NoteRow key={i} icon={ShieldAlert} tone="danger">
                    {msgText(m.zh, m)}
                </NoteRow>
            ))}
            <span className="text-[11px] leading-[16px] font-normal text-text-3">
                {t("verify.blocker-note", "补齐本机 Java 之后回到本页重新开始。")}
            </span>
        </Panel>
    );
}

function RecoverCard() {
    const t = useT();
    return (
        <Panel gap={12}>
            <PanelHead title={t("verify.card-recover", "怎么恢复")} />
            <ol className="flex w-full flex-col gap-1.5 pl-4">
                {[
                    t("verify.rc-1", "重新转换这条任务，交付包会再出一份。"),
                    t("verify.rc-2", "或把交付包放回输出目录原来的位置，再点「开始验证」。"),
                    t("verify.rc-3", "交付包被改名或移动过时，本页认不出新名字。"),
                ].map((x, i) => (
                    <li
                        key={i}
                        className="text-[11px] leading-[16px] font-normal text-text-2 marker:font-mono marker:text-text-3"
                    >
                        {x}
                    </li>
                ))}
            </ol>
            <span className="text-[11px] leading-[16px] font-normal text-text-3">
                {t("verify.rc-note", "六步没有执行，验证副本未创建。")}
            </span>
        </Panel>
    );
}

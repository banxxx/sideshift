/**
 * ConversionTask → ShiftRail 展示映射（Home 实况小窗 / Task 详情页共用）。
 * 站点表恒为定稿四站（见 `railStation`）；「本机执行 installer」是下载站内部一小段，不另起一站。
 * 口径：已越过 emerald、未完成一律灰（当前站只多一圈淡光环）、失败 redstone、取消停在原地不标错；
 * "正在走"由 accent 进行中腿表达，金色不出现在轨道上。
 */
import { Check, RefreshCw, X, type LucideIcon } from "lucide-react";
import type { ConversionTask, FetchTally, PipelineStage, TaskStatus } from "@/lib/types";
import type { Tone } from "@/components/ui";
import { t } from "@/lib/i18n";

/* ---- 轨道展示词表（ShiftRail 与本文件共用；定义放在视图模型层，避免 lib 反向依赖组件） ---- */

/** 轨道站点状态 */
export type RailStageStatus = "pending" | "active" | "done" | "error";

export interface RailLog {
    time?: string;
    stage?: PipelineStage;
    message: string;
    level?: "muted" | "info" | "active" | "error";
}

/** 芯片/状态配色语义（与设计稿状态色一一对应；v5：轨道头部进行态芯片走 accent） */
export type RailTone = "emerald" | "gold" | "redstone" | "accent" | "muted";

const RAIL_ORDER: PipelineStage[] = ["parser", "detector", "downloader", "builder"];

/**
 * 本条任务会不会走「本机执行 installer」这一档。判据与 Rust 侧同源：
 * 开关取自任务快照（不是全局设置，重试不能跟着漂移），且 Fabric 没有安装器可跑 ⇒ 整档跳过。
 */
export function hasInstallerStage(task: ConversionTask): boolean {
    return task.options.installLoaderLocally && task.pack.loader !== "fabric";
}

/**
 * 轨道站点恒为定稿四站：本机执行安装器排在模组取件**之前**，是下载站内部的一小段
 * （30→42），不另起一站——轨道上多一个只在部分任务出现的站，等于让同一个组件有两套几何。
 */
export function railStation(stage: PipelineStage): PipelineStage {
    return stage === "installer" ? "downloader" : stage;
}

/**
 * 本条任务的取件是否真走网络。fetch 计划未落定（阶段 1–2、排队、旧存档）时保守按联网口径。
 */
export function needsNetwork(task: ConversionTask): boolean {
    return task.fetch ? task.fetch.netFiles > 0 : true;
}

/**
 * 阶段 3 计数口径：真联网时按「已下载/需联网」（本地件秒完成，计入会让进度条失真），
 * 全程零流量时按「已取件/全部条目」。
 *
 * `verb` 是**数据不是文案**：调用处拿它比过相等（`verb === "下载"` 决定说「依赖」还是「文件」），
 * 所以这里给的是中文原文；显示时由 `verbLabel` 各自命中自己的词条，比较照原样比。
 */
export function fetchCounts(task: ConversionTask): {
    verb: string;
    done: number;
    total: number;
} {
    const f: FetchTally | undefined = task.fetch;
    if (f && f.netFiles > 0)
        return { verb: "下载", done: task.netDone ?? 0, total: f.netFiles };
    return {
        verb: f ? "取件" : "下载",
        done: task.downloaded ?? 0,
        total: task.total ?? 0,
    };
}

/** 「下载 3/5」/「取件 12/14」——芯片与轨道标题共用的一行文案 */
export function runLabel(task: ConversionTask): string {
    const { verb, done, total } = fetchCounts(task);
    // 排版模板当中文原文写在调用点：三档都走目录命中，中文档逐字与改造前一致；
    // 动词已由 `verbLabel` 翻好，所以英/繁两条目录的值就是同一副槽
    return t("lib.run-label", "{{verb}} {{done}}/{{total}}", {
        verb: verbLabel(verb),
        done,
        total,
    });
}

/** 「下载中 7/12」这类进行态动词的显示形态（比较请用 `fetchCounts().verb` 的原文，别用这个） */
export function verbLabel(verb: string): string {
    return verb === "取件" ? t("lib.collect", "取件") : t("lib.download", "下载");
}

/** 只取计数部分（芯片已带动词时用，如「下载中 7/12」） */
export function runCounts(task: ConversionTask): string {
    const { done, total } = fetchCounts(task);
    return `${done}/${total}`;
}

/** 任务状态 → 芯片文案/色调/图标（任务列表卡与任务详情页共用一套口径）
 *  注意：`label` 存的是中文原文，也就是 i18n 的键 —— 取用处显示时要过一层 `t()`（本仓库目前无人取用它） */
export const STATUS_META: Record<
    TaskStatus,
    { label: string; tone: Tone; icon?: LucideIcon }
> = {
    queued: { label: "排队中", tone: "muted" },
    running: { label: "转换中", tone: "gold", icon: RefreshCw },
    success: { label: "已完成", tone: "emerald", icon: Check },
    failed: { label: "失败", tone: "redstone", icon: X },
    cancelled: { label: "已取消", tone: "muted" },
};

/** 进度条填充类：运行 = 沙金芯 + 同色发光（v5）/ 成功绿 / 失败玫瑰芯 + 发光 / 其余灰 */
export const BAR_COLOR: Record<TaskStatus, string> = {
    queued: "bg-stroke",
    running: "bg-bar-sand shadow-[var(--shadow-bar-sand)]",
    success: "bg-emerald",
    failed: "bg-bar-rose shadow-[var(--shadow-bar-rose)]",
    cancelled: "bg-stroke",
};

export interface TaskRailView {
    statuses: Partial<Record<PipelineStage, RailStageStatus>>;
    status: { label: string; tone: RailTone };
    logs: RailLog[];
    /** 当前阶段在「上一站 → 当前站」这条腿上的完成度 0..1（仅运行态） */
    runFrac?: number;
    /** 站点副标题覆写（零联网任务：下载站如实写成整合包/本地取件） */
    subs?: Partial<Record<PipelineStage, string>>;
    waiting?: { title: string; detail?: string };
}

/**
 * 各阶段在总进度里占的区间（与 Rust task_engine 的写值一一对应：
 * parser 8→15、detector →30、downloader 30→82、builder 84→99）。
 * 本机执行安装器是下载站内部的一小段：预取安装器 jar 30→34、装 34→42，然后模组取件 42→82。
 * 所以 installer 的区间就是下载站整段——腿在装上爬到 23%，接着往下走不会回退。
 */
const STAGE_BAND: Record<PipelineStage, [number, number]> = {
    parser: [0, 15],
    detector: [15, 30],
    downloader: [30, 82],
    installer: [30, 82],
    builder: [84, 99],
};

/** 当前阶段自身区间内的完成度，用于驱动轨道上的进行中腿 */
export function runFraction(task: ConversionTask): number | undefined {
    if (task.status !== "running" || !task.stage) return undefined;
    const [lo, hi] = STAGE_BAND[task.stage];
    return Math.max(0, Math.min(1, (task.progress - lo) / (hi - lo)));
}

export function taskToRail(task: ConversionTask): TaskRailView {
    const station = task.stage ? railStation(task.stage) : null;
    const stageIdx = station ? RAIL_ORDER.indexOf(station) : -1;
    const statuses: Partial<Record<PipelineStage, RailStageStatus>> = {};

    if (task.status === "success") {
        for (const s of RAIL_ORDER) statuses[s] = "done";
    } else if (task.status === "failed") {
        RAIL_ORDER.forEach((s, i) => {
            if (i < stageIdx) statuses[s] = "done";
            else if (i === stageIdx) statuses[s] = "error";
            else statuses[s] = "pending";
        });
    } else if (task.status === "running") {
        RAIL_ORDER.forEach((s, i) => {
            statuses[s] = i < stageIdx ? "done" : i === stageIdx ? "active" : "pending";
        });
    }
    // queued / cancelled：全部留白（pending）

    const tone: RailTone =
        task.status === "failed"
            ? "redstone"
            : task.status === "success"
              ? "emerald"
              : task.status === "cancelled"
                ? "muted"
                : task.status === "running"
                  ? "accent"
                  : "gold";
    const label =
        task.status === "running"
            ? task.stage === "downloader" && task.total != null
              ? t("lib.converting-run", "转换中 · {{run}}", { run: runLabel(task) })
              : t("lib.converting-stage", "转换中 · {{stage}}", {
                    stage: stageLabel(task.stage ?? "parser", needsNetwork(task)),
                })
            : task.status === "queued"
              ? t("lib.queued", "排队中")
              : finishedLabel(task.status);

    const logs: RailLog[] = taskLogs(task);

    return {
        statuses,
        status: { label, tone },
        logs,
        runFrac: runFraction(task),
        subs: needsNetwork(task) ? undefined : { downloader: t("lib.modpack-local", "整合包与本地取件") },
        waiting: logs.length === 0 ? { title: t("lib.waiting-start", "等待开始转换") } : undefined,
    };
}

/**
 * 任务日志 → 轨道控制台行（全量）。
 * Home 缩略小窗与任务详情页读同一份日志：列表接口为轻载裁到末 8 行，
 * 首页实况对选中的任务补一发单任务查询拿全量（见 home-state 的 refresh），
 * 控制台框定高滚动，行数不再影响卡片高度。
 */
export function taskLogs(task: ConversionTask): RailLog[] {
    const finished = task.status !== "queued" && task.status !== "running";
    return task.logs.map((l, i) => ({
        time: l.time,
        stage: l.stage,
        message: l.message,
        level:
            l.level === "error"
                ? "error"
                : finished && i === task.logs.length - 1
                  ? "active"
                  : "info",
    }));
}

/** 收尾三态的轨道标题（轨道上没有进行态：running/queued 由 taskToRail 自己拼） */
function finishedLabel(status: TaskStatus): string {
    const label: Partial<Record<TaskStatus, string>> = {
        success: t("lib.entry-2", "已完成"),
        failed: t("lib.failed", "失败"),
        cancelled: t("lib.canceled", "已取消"),
    };
    return label[status] ?? "";
}

/**
 * 阶段中文短名（芯片文案用，与设计稿"转换中 · 下载 41/146"口径一致）。
 * downloader 站点按是否真联网改口：零流量的包内/本地搬运叫「取件」，不叫「下载」
 */
export function stageLabel(stage: PipelineStage, net = true): string {
    // 表建在函数里：模块顶层建表会把词冻在首次加载的语言上
    const label: Record<PipelineStage, string> = {
        parser: t("lib.parse", "解析"),
        detector: t("lib.detect", "检测"),
        downloader: net ? t("lib.download", "下载") : t("lib.collect", "取件"),
        installer: t("lib.install", "安装"),
        builder: t("lib.build", "构建"),
    };
    return label[stage];
}

/**
 * 进行态短名（进度条芯片）。中文是「阶段名 + 中」，英文是另一种词形（Downloading）——
 * 拼不出来说明它得是独立词条，所以这里整词一个键，不 `${stageLabel()}中`。
 */
function stageIngLabel(stage: PipelineStage, net = true): string {
    if (stage === "downloader" && !net) return t("lib.collecting", "取件中");
    const label: Record<PipelineStage, string> = {
        parser: t("lib.parsing", "解析中"),
        detector: t("lib.detecting", "检测中"),
        downloader: t("lib.downloading", "下载中"),
        installer: t("lib.installing", "安装中"),
        builder: t("lib.building", "构建中"),
    };
    return label[stage];
}

/* ---------------- 任务详情「转换进度」卡（XjfIJ / KdHjU / ZKwyq） ---------------- */

/**
 * 卡内状态芯片：padding[3,10] r99 10/600。
 * running 取当前阶段（"下载中"）+ 金底金字；failed = 灰底红字；cancelled = 灰底灰字；success = 绿底绿字。
 */
export function progressChip(task: ConversionTask): { label: string; tone: Tone; plain: boolean } {
    const net = needsNetwork(task);
    switch (task.status) {
        case "running":
            return {
                label: stageIngLabel(task.stage ?? "parser", net),
                tone: "gold",
                plain: false,
            };
        case "queued":
            return { label: t("lib.queued", "排队中"), tone: "muted", plain: true };
        case "failed":
            return { label: t("lib.failed-2", "已失败"), tone: "redstone", plain: true };
        case "cancelled":
            return { label: t("lib.canceled", "已取消"), tone: "muted", plain: true };
        default:
            return { label: t("lib.entry-2", "已完成"), tone: "emerald", plain: false };
    }
}

/**
 * 卡底「当前站 → 下一站」微型轨道：8px 圆点 + 11/600 标签 + 36×2 连接段 + 灰色下一站。
 * 成功态没有下一站（构建完成即终点）。
 */
export function stageTrack(task: ConversionTask): {
    current: { label: string; tone: Tone };
    next?: string;
} {
    // 这一条画的是「站与站」，本机安装不是站（在下载站里），所以两头都按站点口径取
    const station = task.stage ? railStation(task.stage) : null;
    const idx = station ? RAIL_ORDER.indexOf(station) : -1;
    const net = needsNetwork(task);
    const next =
        idx >= 0 && idx < RAIL_ORDER.length - 1 ? stageLabel(RAIL_ORDER[idx + 1], net) : undefined;
    if (task.status === "success") return { current: { label: t("lib.build-complete", "构建完成"), tone: "emerald" } };
    if (task.status === "failed")
        return {
            current: {
                label: t("lib.stage-failed", "{{stage}}失败", {
                    stage: stageLabel(station ?? "builder", net),
                }),
                tone: "redstone",
            },
            next,
        };
    if (task.status === "cancelled")
        return { current: { label: t("lib.canceled", "已取消"), tone: "muted" }, next };
    return {
        current: { label: stageLabel(station ?? "parser", net), tone: "gold" },
        next,
    };
}

const DOT_COLOR: Record<Tone, string> = {
    emerald: "bg-emerald",
    gold: "bg-gold",
    redstone: "bg-redstone",
    accent: "bg-accent",
    amethyst: "bg-amethyst",
    diamond: "bg-diamond",
    muted: "bg-text-3",
};

const TEXT_COLOR: Record<Tone, string> = {
    emerald: "text-emerald",
    gold: "text-gold",
    redstone: "text-redstone",
    accent: "text-accent",
    amethyst: "text-amethyst",
    diamond: "text-diamond",
    muted: "text-text-3",
};

export const toneDot = (tone: Tone) => DOT_COLOR[tone];
export const toneText = (tone: Tone) => TEXT_COLOR[tone];


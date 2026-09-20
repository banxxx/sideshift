/**
 * ConversionTask → ShiftRail 展示映射（Home 实况小窗 / Task 详情页共用）
 *
 * 设计口径（v11 定稿）：轨道反映任务真实进度——已越过的站点 emerald、
 * 当前站点 gold 激活、未达站点灰色；失败站 redstone；成功=四站全绿；
 * 取消=停在原地（不标错）。
 */
import { Check, RefreshCw, X, type LucideIcon } from "lucide-react";
import type { ConversionTask, FetchTally, PipelineStage, TaskStatus } from "@/lib/types";
import type { RailLog, RailStageStatus, RailTone } from "@/components/features/ShiftRail";
import type { Tone } from "@/components/design/ui";

const ORDER: PipelineStage[] = ["parser", "detector", "downloader", "builder"];

/**
 * 本条任务的取件是否真走网络。fetch 计划未落定（阶段 1–2、排队、旧存档）时保守按联网口径。
 */
export function needsNetwork(task: ConversionTask): boolean {
    return task.fetch ? task.fetch.netFiles > 0 : true;
}

/**
 * 阶段 3 计数口径：真联网时按「已下载/需联网」（本地件秒完成，计入会让进度条失真），
 * 全程零流量时按「已取件/全部条目」。
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
    return `${verb} ${done}/${total}`;
}

/** 只取计数部分（芯片已带动词时用，如「下载中 7/12」） */
export function runCounts(task: ConversionTask): string {
    const { done, total } = fetchCounts(task);
    return `${done}/${total}`;
}

/** 任务状态 → 芯片文案/色调/图标（任务列表卡与任务详情页共用一套口径） */
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

/** 进度条填充色：运行金 / 成功绿 / 失败红 / 其余灰（v4：金色只出现在进度条与芯片上） */
export const BAR_COLOR: Record<TaskStatus, string> = {
    queued: "bg-stroke",
    running: "bg-gold",
    success: "bg-emerald",
    failed: "bg-redstone",
    cancelled: "bg-stroke",
};

export interface TaskRailView {
    statuses: Partial<Record<PipelineStage, RailStageStatus>>;
    status: { label: string; tone: RailTone };
    logs: RailLog[];
    /** 站点副标题覆写（零联网任务：下载站如实写成整合包/本地取件） */
    subs?: Partial<Record<PipelineStage, string>>;
    waiting?: { title: string; detail?: string };
}

export function taskToRail(task: ConversionTask): TaskRailView {
    const stageIdx = task.stage ? ORDER.indexOf(task.stage) : -1;
    const statuses: Partial<Record<PipelineStage, RailStageStatus>> = {};

    if (task.status === "success") {
        for (const s of ORDER) statuses[s] = "done";
    } else if (task.status === "failed") {
        ORDER.forEach((s, i) => {
            if (i < stageIdx) statuses[s] = "done";
            else if (i === stageIdx) statuses[s] = "error";
            else statuses[s] = "pending";
        });
    } else if (task.status === "running") {
        ORDER.forEach((s, i) => {
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
                : "gold";
    const label =
        task.status === "running"
            ? task.stage === "downloader" && task.total != null
              ? `转换中 · ${runLabel(task)}`
              : `转换中 · ${stageLabel(task.stage ?? "parser", needsNetwork(task))}`
            : task.status === "queued"
              ? "排队中"
              : FINISHED_LABEL[task.status];

    const logs: RailLog[] = taskLogs(task).slice(-3);

    return {
        statuses,
        status: { label, tone },
        logs,
        subs: needsNetwork(task) ? undefined : { downloader: "整合包与本地取件" },
        waiting: logs.length === 0 ? { title: "等待开始转换" } : undefined,
    };
}

/**
 * 任务日志 → 轨道控制台行（全量）。
 * Home 缩略小窗只取末 3 行（taskToRail），任务详情页传全量——控制台高度随行数增长。
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

const FINISHED_LABEL: Record<string, string> = {
    success: "已完成",
    failed: "失败",
    cancelled: "已取消",
};

/**
 * 阶段中文短名（芯片文案用，与设计稿"转换中 · 下载 41/146"口径一致）。
 * downloader 站点按是否真联网改口：零流量的包内/本地搬运叫「取件」，不叫「下载」
 */
export function stageLabel(stage: PipelineStage, net = true): string {
    if (stage === "downloader" && !net) return "取件";
    return { parser: "解析", detector: "检测", downloader: "下载", builder: "构建" }[stage];
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
                label: `${stageLabel(task.stage ?? "parser", net)}中`,
                tone: "gold",
                plain: false,
            };
        case "queued":
            return { label: "排队中", tone: "muted", plain: true };
        case "failed":
            return { label: "已失败", tone: "redstone", plain: true };
        case "cancelled":
            return { label: "已取消", tone: "muted", plain: true };
        default:
            return { label: "已完成", tone: "emerald", plain: false };
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
    const idx = task.stage ? ORDER.indexOf(task.stage) : -1;
    const net = needsNetwork(task);
    const next =
        idx >= 0 && idx < ORDER.length - 1 ? stageLabel(ORDER[idx + 1], net) : undefined;
    if (task.status === "success") return { current: { label: "构建完成", tone: "emerald" } };
    if (task.status === "failed")
        return {
            current: {
                label: `${stageLabel(task.stage ?? "builder", net)}失败`,
                tone: "redstone",
            },
            next,
        };
    if (task.status === "cancelled") return { current: { label: "已取消", tone: "muted" }, next };
    return {
        current: { label: stageLabel(task.stage ?? "parser", net), tone: "gold" },
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


/**
 * Shift Rail 转换轨道（SS.pen Home 帧 `AGkr7`，v11 定稿）
 *
 * 纯展示组件：四站（解析→检测→下载→构建）+ 轨道线 + 两端芯片 + 日志控制台。
 * 不感知业务数据来源——调用方把 TaskStatus/PipelineStage 映射成 stations/logs 传入，
 * 因此 Home 实况小窗、Task 详情页、任务列表都能复用同一份轨道。
 *
 * 站点四态（设计稿定稿）：
 * - done    $emerald-dim 底 + $emerald 描边/图标（图标固定 check）
 * - active  $gold-dim 底 + $gold 描边/图标（图标为站点自身图标）
 * - error   $redstone-dim 底 + $redstone 描边/图标（x）
 * - pending $surface-2 底 + $stroke 描边 + $text-3 图标
 * 轨道线只有两态（用户定稿，无金色进行中段）：底轨 $rail-track，
 * 已完成段 $emerald，宽度按"上一站中心→当前站中心"逐段推进。
 */
import { Archive, Check, Download, FileSearch, Hammer, Radar, Server, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { PipelineStage } from "@/lib/types";
import { formatClock } from "@/lib/format";

/** 轨道站点状态 */
export type RailStageStatus = "pending" | "active" | "done" | "error";

export interface RailLog {
    time?: string;
    stage?: PipelineStage;
    message: string;
    level?: "muted" | "info" | "active" | "error";
}

/** 芯片/状态配色语义（与设计稿状态色一一对应） */
export type RailTone = "emerald" | "gold" | "redstone" | "muted";

interface ShiftRailProps {
    /** 各站点状态；缺省按 pending */
    statuses: Partial<Record<PipelineStage, RailStageStatus>>;
    /** 头部右侧状态芯片；不传则不显示 */
    status?: { label: string; tone: RailTone };
    /** 日志行；为空时显示 waiting 占位行 */
    logs?: RailLog[];
    /** 控制台占位文案（如"等待开始转换"） */
    waiting?: { title: string; detail?: string };
    /** 是否显示底部"查看任务详情"链接 */
    onOpenTask?: () => void;
    className?: string;
}

/** 四站静态定义：图标/中文名/副标题（副标题 mono 小字，与设计稿逐字对应） */
export const RAIL_STAGES: Array<{
    stage: PipelineStage;
    label: string;
    sub: string;
    icon: typeof Archive;
}> = [
    { stage: "parser", label: "解析", sub: "读取 mrpack 清单", icon: FileSearch },
    { stage: "detector", label: "检测", sub: "识别加载器与版本", icon: Radar },
    { stage: "downloader", label: "下载", sub: "拉取服务端依赖", icon: Download },
    { stage: "builder", label: "构建", sub: "生成服务端实例", icon: Hammer },
];

/** 站点状态 → 配色/图标（done 默认换成 check，前沿站例外见下） */
const STATION_STYLE: Record<
    RailStageStatus,
    { box: string; iconColor: string }
> = {
    done: { box: "bg-emerald-dim border-emerald", iconColor: "text-emerald" },
    active: { box: "bg-gold-dim border-gold", iconColor: "text-gold" },
    error: { box: "bg-redstone-dim border-redstone", iconColor: "text-redstone" },
    pending: { box: "bg-surface-2 border-stroke", iconColor: "text-text-3" },
};

/**
 * 完成站图标规则（SS.pen Home·转换中 `Em5yO` 定稿）：
 * 已被后续站超越的完成站显示 check，而"刚完成、进度线正停在其上"的那一站
 * 保留自身图标（如检测完成但下载进行中时检测仍显示 radar）；
 * 全部完成（成功态）则四站统一 check。
 */
function doneShowsOwnIcon(list: RailStageStatus[], i: number): boolean {
    if (list[i] !== "done") return false;
    const lastDone = list.lastIndexOf("done");
    const hasFrontier = list.some(
        (s) => s === "active" || s === "pending" || s === "error"
    );
    return i === lastDone && hasFrontier;
}

const CHIP_TONE: Record<RailTone, string> = {
    emerald: "bg-emerald-dim text-emerald",
    gold: "bg-gold-dim text-gold",
    redstone: "bg-redstone-dim text-redstone",
    muted: "bg-surface-2 text-text-3",
} as const;

const LOG_LEVEL_COLOR = {
    muted: "text-text-3",
    info: "text-text-2",
    active: "text-gold",
    error: "text-redstone",
} as const;

/**
 * 轨道几何：设计稿 rail-body 内宽 884，站点中心 x=130/343/557/770，底轨 130→770（640），
 * 线 y=44、站点 38×38（y=26）、标签 y=74。此处一律折算成百分比，
 * 使 1200 窗口下与设计稿逐像素对齐，窗口缩放时等比收缩而不重叠。
 */
const RAIL_W = 884;
const LINE_START = 130;
const LINE_SPAN = 640;
const STATION_CENTERS = [130, 343, 557, 770].map((x) => (x / RAIL_W) * 100);
const LINE_LEFT = (LINE_START / RAIL_W) * 100;
const LINE_WIDTH = (LINE_SPAN / RAIL_W) * 100;
/** 标签宽度 190/884，居中挂在站点下方 */
const LABEL_WIDTH = (190 / RAIL_W) * 100;

/**
 * 已完成段宽度（占底轨比例）：推进到"最后一个 done 站"的中心，与设计稿一致
 * （Home 帧 parser+detector 完成 → 213/640；Ready 帧无完成站 → 0）。
 * 用户定稿"轨道线只有两态、无金色进行中段"，故站内的实时进度不画在线上。
 */
function doneLineFraction(statuses: RailStageStatus[]): number {
    let k = 0;
    while (k < statuses.length && statuses[k] === "done") k++;
    if (k === 0) return 0;
    if (k === statuses.length) return 1;
    // 百分比域直接相除即得占底轨比例
    return (STATION_CENTERS[k - 1] - STATION_CENTERS[0]) / LINE_WIDTH;
}

export function ShiftRail({
                              statuses,
                              status,
                              logs = [],
                              waiting,
                              onOpenTask,
                              className,
                          }: ShiftRailProps) {
    const list = RAIL_STAGES.map((s) => statuses[s.stage] ?? "pending");
    const doneFrac = doneLineFraction(list);

    return (
        <section
            className={cn(
                "bg-surface border border-stroke rounded-[12px] p-5 flex flex-col gap-3 w-full",
                className
            )}
        >
            {/* 头部：轨道标题 + 状态芯片 */}
            <header className="flex items-center justify-between">
                <span className="font-mono text-[11px] font-semibold tracking-[1.2px] text-text-3">
                    SHIFT RAIL · 转换轨道
                </span>
                {status && (
                    <span
                        className={cn(
                            "flex items-center gap-1.5 rounded-full px-2.5 py-1 font-mono text-[11px] font-medium",
                            CHIP_TONE[status.tone]
                        )}
                    >
                        <span className="size-1.5 rounded-full bg-current" />
                        {status.label}
                    </span>
                )}
            </header>

            {/* 轨道主体：设计稿为固定几何（w884 h126），这里等比用 px 布局 */}
            <div className="relative h-[126px]">
                {/* 端点芯片 */}
                <EndChip icon={Archive} label="客户端包" className="left-1.5 top-[29px]" />
                <EndChip
                    icon={Server}
                    label="服务端包"
                    iconClass="text-emerald"
                    className="right-1.5 top-[29px]"
                />

                {/* 底轨 + 已完成段（几何按设计稿折算为百分比） */}
                <span
                    className="absolute h-[3px] rounded-sm bg-rail-track"
                    style={{ left: `${LINE_LEFT}%`, top: 44, width: `${LINE_WIDTH}%` }}
                />
                {doneFrac > 0 && (
                    <span
                        className="absolute h-[3px] rounded-sm bg-emerald transition-[width] duration-500"
                        style={{
                            left: `${LINE_LEFT}%`,
                            top: 44,
                            width: `${LINE_WIDTH * doneFrac}%`,
                        }}
                    />
                )}

                {/* 四站：站点盒 + 名称/副标题，整列以轨道中心对齐 */}
                {RAIL_STAGES.map((s, i) => {
                    const st = list[i];
                    const style = STATION_STYLE[st];
                    const Icon = s.icon;
                    return (
                        <div
                            key={s.stage}
                            className="absolute -translate-x-1/2 flex flex-col items-center gap-2.5"
                            style={{ left: `${STATION_CENTERS[i]}%`, width: `${LABEL_WIDTH}%`, top: 26 }}
                        >
                            <span
                                className={cn(
                                    "size-[38px] shrink-0 rounded-[10px] border flex items-center justify-center",
                                    style.box
                                )}
                            >
                                {st === "done" && !doneShowsOwnIcon(list, i) ? (
                                    <Check className={cn("size-4", style.iconColor)} />
                                ) : st === "error" ? (
                                    <X className={cn("size-4", style.iconColor)} />
                                ) : (
                                    <Icon
                                        className={cn(
                                            "size-4",
                                            style.iconColor,
                                            st === "active" && "animate-pulse"
                                        )}
                                    />
                                )}
                            </span>
                            <div className="flex flex-col items-center gap-0.5 w-full">
                                <span
                                    className={cn(
                                        "text-xs font-semibold text-center",
                                        st === "pending" ? "text-text-2" : "text-text-1"
                                    )}
                                >
                                    {s.label}
                                </span>
                                <span className="font-mono text-[10px] text-text-3 text-center w-full">
                                    {s.sub}
                                </span>
                            </div>
                        </div>
                    );
                })}
            </div>

            {/* 底部链接行（正常流成员，仅当有可查看的任务） */}
            {onOpenTask && (
                <footer className="flex justify-end">
                    <button
                        onClick={onOpenTask}
                        className="text-[11px] font-semibold text-accent hover:underline"
                    >
                        查看任务详情 →
                    </button>
                </footer>
            )}

            {/* 日志控制台：高度随行数自适应（设计稿无 min-height），底色 $bg-app */}
            <div className="bg-bg-app border border-stroke-soft rounded-lg px-4 py-2.5 flex flex-col gap-1.5">
                {logs.length === 0 && waiting ? (
                    <p className="flex items-center gap-2 font-mono text-[11px]">
                        <span className="font-semibold text-text-3">{waiting.title}</span>
                        {waiting.detail && (
                            <span className="text-text-3">{waiting.detail}</span>
                        )}
                    </p>
                ) : (
                    logs.map((l, i) => (
                        <p key={i} className="flex items-center gap-2 font-mono min-w-0">
                            <span className="text-[10px] text-text-3 shrink-0">
                                {l.time ?? formatClock()}
                            </span>
                            {l.stage && (
                                <span className="text-[10px] font-semibold text-amethyst shrink-0">
                                    [{l.stage}]
                                </span>
                            )}
                            <span
                                className={cn(
                                    "text-[11px] truncate",
                                    LOG_LEVEL_COLOR[l.level ?? "info"]
                                )}
                            >
                                {l.message}
                            </span>
                        </p>
                    ))
                )}
            </div>
        </section>
    );
}

/** 轨道两端芯片：surface-2 底 + stroke 描边（客户端包 / 服务端包） */
function EndChip({
                     icon: Icon,
                     label,
                     className,
                     iconClass = "text-amethyst",
                 }: {
    icon: typeof Archive;
    label: string;
    className?: string;
    iconClass?: string;
}) {
    return (
        <span
            className={cn(
                "absolute h-8 flex items-center gap-[7px] rounded-lg border border-stroke bg-surface-2 px-2.5 text-[11px] font-semibold text-text-2",
                className
            )}
        >
            <Icon className={cn("size-[13px]", iconClass)} />
            {label}
        </span>
    );
}

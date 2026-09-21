/**
 * Shift Rail 转换轨道（SS.pen Home 帧 `AGkr7`，v11 定稿）
 *
 * 纯展示组件：四站（解析→检测→下载→构建）+ 轨道线 + 两端芯片 + 日志控制台。
 * 不感知业务数据来源——调用方把 TaskStatus/PipelineStage 映射成 stations/logs 传入，
 * 因此 Home 实况小窗、Task 详情页、任务列表都能复用同一份轨道。
 *
 * 站点三态（用户定稿：未完成一律灰，轨道上不用黄色）：
 * - done    $emerald-dim 底 + $emerald 描边/图标（图标固定 check）
 * - active  与 pending 同灰底灰图标，只加一圈很淡的品牌色光环 + 呼吸
 * - pending $surface-2 底 + $stroke 描边 + $text-3 图标
 * - error   $redstone-dim 底 + $redstone 描边/图标（x）
 * 轨道三段：底轨 $rail-track，已完成段 $emerald（按"上一站中心→当前站中心"逐段推进），
 * **进行中腿** $accent——按当前阶段的真实完成度填充，上面跑一条高光带（rail-flow）
 * 并在前沿放一颗带光晕的亮点，解决「只有图标在变、线不动」。
 *
 * 纵向几何很紧（默认 1200×800 下首页不能再高）：rail-body 106 = 站点顶留位 26 + 盒 38
 * + 10 + 名称 16 + 2 + 副标题 14，日志盒 80 = padding 20 + 3×16 行 + 2×6 间距。
 * 所有 11/10px 小字都显式写 leading——html 的 line-height:24px 会白撑高每一行。
 */
import { useRef } from "react";
import {
    Archive,
    Check,
    Download,
    FileSearch,
    Hammer,
    Radar,
    Server,
    X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { PipelineStage } from "@/lib/types";
import { formatClock } from "@/lib/format";
import { useLogFollow } from "@/lib/log-view";
import type { RailLog, RailStageStatus, RailTone } from "@/lib/rail-view";
import { LogCopyButton } from "@/components/shared/LogCopyButton";

interface ShiftRailProps {
    /** 各站点状态；缺省按 pending */
    statuses: Partial<Record<PipelineStage, RailStageStatus>>;
    /** 头部右侧状态芯片；不传则不显示 */
    status?: { label: string; tone: RailTone };
    /** 日志行；为空时显示 waiting 占位行 */
    logs?: RailLog[];
    /** 控制台占位文案（如"等待开始转换"） */
    waiting?: { title: string; detail?: string };
    /** 站点副标题覆写：零联网任务把「拉取服务端依赖」换成如实文案（站名按设计稿不动） */
    subs?: Partial<Record<PipelineStage, string>>;
    /** 复制日志时的首行上下文（包名/任务号） */
    clipHeader?: string;
    /** 当前阶段在「上一站中心 → 当前站中心」这条腿上的完成度 0..1；非运行态不传 */
    runFrac?: number;
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
    // 未完成就是灰：进行中靠轨道上跑动的那段表达，站点不再涂金（用户定稿：轨道上不要黄色）
    active: { box: "bg-surface-2 border-stroke ring-2 ring-accent/25", iconColor: "text-text-3" },
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
 * 轨道几何（流体·对称）：两端芯片按自身内容宽排在行两端，芯片与轨道区之间固定
 * 39px 行距 = 站点盒半径 19 + 视觉留白 20。于是「芯片外缘 → 站点盒外缘」左右
 * 都正好 20px，且芯片文案变长也只吞行距、两端依旧等值——设计稿那组 130/114 的
 * 绝对内缩左 20px 右 4px，本来就不对称。
 * 四站等分轨道区（中心落在区内 0/33.3/66.7/100%）、底轨跨满整区，站距严格相等。
 * 1200 窗口下首站中心 = 6 + 芯片 85 + 39 = 130，仍与设计稿的 130 对齐。
 */
const STATION_CENTERS = [0, 100 / 3, 200 / 3, 100];
const LINE_LEFT = 0;
const LINE_WIDTH = 100;
/** 标签格 = 一格站距，居中挂在站点下方，相邻格刚好首尾相接 */
const LABEL_WIDTH = 100 / 3;

/**
 * 已完成段宽度（占底轨比例）：推进到"最后一个 done 站"的中心，与设计稿一致
 * （Home 帧 parser+detector 完成 → 213/640；Ready 帧无完成站 → 0）。
 * 已完成段只用 $emerald（无金色），站内的实时进度交给「进行中腿」表达。
 */
function doneLineFraction(statuses: RailStageStatus[]): number {
    let k = 0;
    while (k < statuses.length && statuses[k] === "done") k++;
    if (k === 0) return 0;
    if (k === statuses.length) return 1;
    // 百分比域直接相除即得占底轨比例
    return (STATION_CENTERS[k - 1] - STATION_CENTERS[0]) / LINE_WIDTH;
}

/**
 * 进行中腿：从已完成段前沿走到「第一个未完成站」中心。
 * 底轨本身覆盖了整段，所以这里只需给出起终点的百分比即可。
 */
function runningLeg(
    statuses: RailStageStatus[],
    doneFrac: number
): { left: number; width: number } | null {
    const nextIdx = statuses.findIndex((s) => s !== "done");
    if (nextIdx === -1) return null;
    const left = LINE_LEFT + LINE_WIDTH * doneFrac;
    const width = STATION_CENTERS[nextIdx] - left;
    return width > 0.5 ? { left, width } : null;
}

export function ShiftRail({
                              statuses,
                              status,
                              logs = [],
                              waiting,
                              subs,
                              clipHeader,
                              runFrac,
                              onOpenTask,
                              className,
                          }: ShiftRailProps) {
    const list = RAIL_STAGES.map((s) => statuses[s.stage] ?? "pending");
    const doneFrac = doneLineFraction(list);
    const leg = runningLeg(list, doneFrac);
    const runPct = leg ? leg.width * Math.max(0, Math.min(1, runFrac ?? 0)) : 0;
    const logBoxRef = useRef<HTMLDivElement>(null);
    useLogFollow(logBoxRef, logs.length);

    return (
        <section
            className={cn(
                "bg-surface border border-stroke rounded-[12px] p-5 flex flex-col gap-3 w-full",
                className
            )}
        >
            {/* 头部：轨道标题 + 状态芯片（显式 leading，否则继承 html 的 24px 行高白撑高） */}
            <header className="flex items-center justify-between">
                <span className="font-mono text-[11px] leading-[16px] font-semibold tracking-[1.2px] text-text-3">
                    SHIFT RAIL · 转换轨道
                </span>
                {status && (
                    <span
                        className={cn(
                            "flex items-center rounded-full px-2.5 py-1 font-mono text-[11px] leading-[14px] font-semibold",
                            CHIP_TONE[status.tone]
                        )}
                    >
                        {status.label}
                    </span>
                )}
            </header>

            {/* 轨道主体：芯片(内容宽) + 39px 行距 + 轨道区(flex-1) + 39px + 芯片。
                轨道区内所有百分比都相对自身宽度；站点盒半径 19 会吃掉一半行距，
                所以芯片与站点盒的视觉净间距左右都是 20px。
                纵向 106 = 站点顶留位 26 + 盒 38 + 10 + 名称 16 + 2 + 副标题 14 */}
            <div className="flex h-[106px] gap-[39px] px-1.5">
                <EndChip icon={Archive} label="客户端包" className="mt-[29px] shrink-0" />
                <div className="relative min-w-0 flex-1">
                    {/* 底轨 + 已完成段 */}
                    <span
                        className="absolute h-[3px] rounded-full bg-rail-track"
                        style={{ left: `${LINE_LEFT}%`, top: 44, width: `${LINE_WIDTH}%` }}
                    />
                    {doneFrac > 0 && (
                        <span
                            className="absolute h-[3px] rounded-full bg-emerald transition-[width] duration-500"
                            style={{
                                left: `${LINE_LEFT}%`,
                                top: 44,
                                width: `${LINE_WIDTH * doneFrac}%`,
                            }}
                        />
                    )}

                    {/* 进行中腿：淡色待走路 + accent 流动段（宽度 = 当前阶段完成度）+ 头部亮点。
                        这是「线在走动」的唯一载体——站点盒保持灰色，不靠涂色表意 */}
                    {leg && runFrac != null && (
                        <>
                            <span
                                className="absolute h-[3px] rounded-full bg-accent/12"
                                style={{
                                    left: `${leg.left}%`,
                                    top: 44,
                                    width: `${leg.width}%`,
                                }}
                            />
                            <span
                                className="rail-flow absolute h-[3px] rounded-full transition-[width] duration-700 ease-linear"
                                style={{
                                    left: `${leg.left}%`,
                                    top: 44,
                                    width: `${runPct}%`,
                                }}
                            />
                            <span
                                className="rail-head absolute size-[7px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent transition-[left] duration-700 ease-linear"
                                style={{
                                    left: `${leg.left + runPct}%`,
                                    top: 45.5,
                                }}
                            />
                        </>
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
                                <div className="flex w-full flex-col items-center gap-0.5">
                                    <span
                                        className={cn(
                                            "text-xs leading-[16px] font-semibold text-center",
                                            st === "pending" ? "text-text-2" : "text-text-1"
                                        )}
                                    >
                                        {s.label}
                                    </span>
                                    <span className="font-mono text-[10px] leading-[14px] font-normal text-center text-text-3 w-full">
                                        {subs?.[s.stage] ?? s.sub}
                                    </span>
                                </div>
                            </div>
                        );
                    })}
                </div>
                <EndChip
                    icon={Server}
                    label="服务端包"
                    iconClass="text-emerald"
                    className="mt-[29px] shrink-0"
                />
            </div>

            {/* 底部链接行（正常流成员，仅当有可查看的任务） */}
            {onOpenTask && (
                <footer className="flex justify-end">
                    <button
                        onClick={onOpenTask}
                        className="text-[11px] leading-[16px] font-semibold text-accent hover:underline"
                    >
                        查看任务详情 →
                    </button>
                </footer>
            )}

            {/* 日志控制台：与任务详情页读同一份日志尾，框内滚动、高度不顶卡。
                12.5vh 上限 100：1200×800 下正好 100（≈4 行），窗口变矮时跟着收，
                不至于把整张轨道卡挤出默认视口。滚动条样式见 App.css，新行自动贴底 */}
            <div className="relative">
                <div
                    ref={logBoxRef}
                    className="log-scroll h-[clamp(56px,12.5vh,100px)] overflow-y-auto rounded-lg border border-stroke-soft bg-bg-app px-4 py-2.5 flex flex-col gap-1.5"
                >
                    {logs.length === 0 && waiting ? (
                        <p className="flex items-center gap-2 font-mono text-[11px] leading-[16px]">
                            <span className="font-semibold text-text-3">{waiting.title}</span>
                            {waiting.detail && (
                                <span className="text-text-3">{waiting.detail}</span>
                            )}
                        </p>
                    ) : (
                        logs.map((l, i) => (
                            <p key={i} className="flex items-center gap-2 font-mono min-w-0 leading-[16px]">
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
                {logs.length > 0 && (
                    <LogCopyButton
                        variant="floating"
                        logs={logs}
                        header={clipHeader}
                        className="absolute right-3 top-2.5 h-6 w-6 rounded-md border border-stroke-soft bg-surface text-text-3 hover:bg-surface-2 hover:text-text-1"
                    />
                )}
            </div>
        </section>
    );
}

/** 轨道两端芯片：surface-2 底 + stroke 描边（客户端包 / 服务端包），排在轨道主体两端 */
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
                "h-8 flex items-center gap-[7px] rounded-lg border border-stroke bg-surface-2 px-2.5 text-[11px] leading-[14px] font-semibold text-text-2",
                className
            )}
        >
            <Icon className={cn("size-[13px]", iconClass)} />
            {label}
        </span>
    );
}

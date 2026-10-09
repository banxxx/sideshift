/**
 * Shift Rail 转换轨道（v5 几何）：站点/轨道各占一份弹性宽度的横向 flex——站点列（圆节点 + 名称/副标）
 * 与轨道段（3px 线）交替排列，四站等分、站距由布局天然保证（不再用绝对百分比摆站）。
 * 站点三态（v5 配色）：done = accent 实心圆 + 白图标 + accent 光环影；active = $surface 圆 + 2px accent 描边；
 * error 红石淡底；pending = $surface-2 圆 + 灰图标。「刚完成、进度线正停在其上」的站保留自身图标（见 doneShowsOwnIcon）。
 * 已完成段（站与站之间的整段轨道）填 accent；进行中腿在同一根轨道上按当前阶段完成度推进，上跑 rail-flow 高光带。
 * 日志控制台已拆出为独立卡 RailConsole（v5：ConsoleCard 与轨道卡平级）。
 */
import { Fragment } from "react";
import {
    Archive,
    ArrowRight,
    Check,
    Download,
    FileSearch,
    Hammer,
    Radar,
    Server,
    X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { t, useT } from "@/lib/i18n";
import type { PipelineStage } from "@/lib/types";
import type { RailStageStatus, RailTone } from "@/lib/rail-view";

interface ShiftRailProps {
    /** 各站点状态；缺省按 pending */
    statuses: Partial<Record<PipelineStage, RailStageStatus>>;
    /** 头部右侧状态芯片；不传则不显示 */
    status?: { label: string; tone: RailTone };
    /** 站点副标题覆写：零联网任务把「拉取服务端依赖」换成如实文案（站名按设计稿不动） */
    subs?: Partial<Record<PipelineStage, string>>;
    /** 当前阶段在「上一站 → 当前站」这条轨道上的完成度 0..1；非运行态不传 */
    runFrac?: number;
    /** 是否显示头部"查看任务详情"链接 */
    onOpenTask?: () => void;
    className?: string;
}

/** 四站静态定义：图标 + 文案（副标题 mono 小字，与设计稿逐字对应）。
 *  表建在函数里、每格一条 `t(字面量)`：顶层建表会把词冻在首次加载的语言上（切语言不再跟）。 */
export function railStages(): Array<{
    stage: PipelineStage;
    label: string;
    sub: string;
    icon: typeof Archive;
}> {
    return [
        { stage: "parser", label: t("lib.parse", "解析"), sub: t("common.read-mrpack", "读取 mrpack 清单"), icon: FileSearch },
        { stage: "detector", label: t("lib.detect", "检测"), sub: t("common.loaders-version", "识别加载器与版本"), icon: Radar },
        { stage: "downloader", label: t("lib.download", "下载"), sub: t("common.server-deps", "拉取服务端依赖"), icon: Download },
        { stage: "builder", label: t("lib.build", "构建"), sub: t("common.server-instance", "生成服务端实例"), icon: Hammer },
    ];
}

/** 站点状态 → 配色/图标（v5：done/active 都是 accent 家族，pending 全灰） */
const STATION_STYLE: Record<
    RailStageStatus,
    { box: string; iconColor: string }
> = {
    done: { box: "bg-accent shadow-[var(--shadow-node)]", iconColor: "text-accent-ink" },
    active: { box: "bg-surface border-2 border-accent", iconColor: "text-accent" },
    error: { box: "bg-redstone-dim border border-redstone", iconColor: "text-redstone" },
    pending: { box: "bg-surface-2", iconColor: "text-text-3" },
};

/**
 * 完成站图标规则（v5 沿用）：已被后续站超越的完成站显示 check，而"刚完成、进度线正停在其上"的
 * 那一站保留自身图标（如检测完成但下载进行中时检测仍显示 radar）；全部完成（成功态）则四站统一 check。
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
    accent: "bg-accent-dim text-accent",
    muted: "bg-surface-2 text-text-3",
} as const;

/** 前缀连续 done 的数量：它们右侧的站间轨道全部填 accent */
function doneCount(list: RailStageStatus[]): number {
    let k = 0;
    while (k < list.length && list[k] === "done") k++;
    return k;
}

export function ShiftRail({
                              statuses,
                              status,
                              subs,
                              runFrac,
                              onOpenTask,
                              className,
                          }: ShiftRailProps) {
    const t = useT();
    const stages = railStages();
    const list = stages.map((s) => statuses[s.stage] ?? "pending");
    const done = doneCount(list);
    // 进行中腿画在「最后一个 done 站 → 它后面那站」这段轨道上（即第 done-1 段，0 起）
    const legTrack = done > 0 && done < list.length ? done - 1 : -1;
    const frac = Math.max(0, Math.min(1, runFrac ?? 0));

    return (
        <section
            className={cn(
                "card-frost rounded-[12px] p-5 flex flex-col gap-3 w-full",
                className
            )}
        >
            {/* 头部：左 = 轨道标题 + 两端包胶囊（客户端包 → 服务端包）；
                右 = 状态芯片 + 查看任务详情（v5 把两者收进同一行，显式 leading 防继承 24px 行高） */}
            <header className="flex items-center justify-between gap-3">
                <div className="flex min-w-0 items-center gap-2.5">
                    <span className="font-mono text-[11px] leading-[16px] font-semibold tracking-[1.2px] text-text-3 shrink-0">
                        {t("common.conversion-rail", "转换轨道")}
                    </span>
                    <EndChip icon={Archive} label={t("common.client-pack", "客户端包")} />
                    <ArrowRight className="size-3 shrink-0 text-text-3" strokeWidth={2.5} />
                    <EndChip
                        icon={Server}
                        label={t("common.server-pack", "服务端包")}
                        accent
                    />
                </div>
                <div className="flex shrink-0 items-center gap-3">
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
                    {onOpenTask && (
                        <button
                            onClick={onOpenTask}
                            className="text-[11px] leading-[16px] font-semibold text-text-2 hover:text-text-1"
                        >
                            {t("common.view-task", "查看任务详情")} →
                        </button>
                    )}
                </div>
            </header>

            {/* 轨道主体：站点列与轨道段交替，各占 flex-1（v5 几何：四站等分由布局保证）。
                轨道线顶在 16px：站点盒 34 的圆心 17，线芯 17.5，差半像素肉眼不可分。
                74 = 盒 34 + 间距 8 + 名称 16 + 2 + 副标 14 */}
            <div className="flex h-[74px]">
                {stages.map((s, i) => {
                    const st = list[i];
                    const style = STATION_STYLE[st];
                    const Icon = s.icon;
                    return (
                        <Fragment key={s.stage}>
                            {i > 0 && (
                                <TrackSegment
                                    done={i < done}
                                    runningFrac={i - 1 === legTrack && runFrac != null ? frac : undefined}
                                />
                            )}
                            <div className="min-w-0 flex-1 flex flex-col items-center gap-2">
                                <span
                                    className={cn(
                                        "size-[34px] shrink-0 rounded-full border-transparent flex items-center justify-center",
                                        style.box
                                    )}
                                >
                                    {st === "done" && !doneShowsOwnIcon(list, i) ? (
                                        <Check className={cn("size-[14px]", style.iconColor)} strokeWidth={2.5} />
                                    ) : st === "error" ? (
                                        <X className={cn("size-[14px]", style.iconColor)} />
                                    ) : (
                                        <Icon
                                            className={cn(
                                                "size-[14px]",
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
                        </Fragment>
                    );
                })}
            </div>
        </section>
    );
}

/**
 * 站间轨道段：底轨 rail-track；已完成段整段 accent；
 * 进行中腿（runningFrac 给出时）按完成度推进 accent 填充 + rail-flow 高光带。
 */
function TrackSegment({
                          done,
                          runningFrac,
                      }: {
    done: boolean;
    runningFrac?: number;
}) {
    return (
        <div className="min-w-0 flex-1 pt-4">
            <div className="relative h-[3px] rounded-full bg-rail-track overflow-hidden">
                {done && <span className="absolute inset-0 rounded-full bg-accent" />}
                {runningFrac != null && runningFrac > 0 && (
                    <span
                        className="rail-flow absolute left-0 top-0 h-[3px] rounded-full transition-[width] duration-700 ease-linear"
                        style={{ width: `${runningFrac * 100}%` }}
                    />
                )}
            </div>
        </div>
    );
}

/** 轨道两端胶囊：客户端包（$surface-2 纱底）/ 服务端包（accent 淡底），排在头部轨道标题右侧 */
function EndChip({
                     icon: Icon,
                     label,
                     accent,
                 }: {
    icon: typeof Archive;
    label: string;
    accent?: boolean;
}) {
    return (
        <span
            className={cn(
                "flex shrink-0 items-center gap-1.5 rounded-full px-[9px] py-1 text-[11px] leading-[16px] font-semibold",
                accent ? "bg-accent-dim text-accent" : "bg-surface-2 text-text-2"
            )}
        >
            <Icon className={cn("size-[13px]", accent ? "text-accent" : "text-text-2")} />
            {label}
        </span>
    );
}

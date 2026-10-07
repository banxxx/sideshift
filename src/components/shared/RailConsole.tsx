/**
 * 运行日志控制台卡（v5 ConsoleCard）：与 ShiftRail 平级的独立卡——卡头「运行日志 + 时间芯片 + 复制」，
 * 日志行直接排在卡面上（不再有内嵌的描边小盒）。
 * 时间芯片取末行日志的 HH:MM（没有日志时落到本地时钟），是「这份日志新鲜到什么时候」的一眼读数。
 * 行为与原轨道内嵌控制台一致：log-scroll 定高滚动、新行贴底（useLogFollow）、复制走 LogCopyButton。
 */
import { useRef } from "react";
import { cn } from "@/lib/utils";
import { tSource, useT } from "@/lib/i18n";
import { formatClock } from "@/lib/format";
import { useLogFollow } from "@/lib/log-view";
import type { RailLog } from "@/lib/rail-view";
import { LogCopyButton } from "@/components/shared/LogCopyButton";

/** 日志 stage 标签分色（v5：[parser] accent / [detector] emerald / [downloader] gold），builder 沿用 amethyst */
const STAGE_COLOR: Record<string, string> = {
    parser: "text-accent",
    detector: "text-emerald",
    downloader: "text-gold",
    installer: "text-gold",
    builder: "text-amethyst",
};

const LOG_LEVEL_COLOR = {
    muted: "text-text-3",
    info: "text-text-2",
    active: "text-gold",
    error: "text-redstone",
} as const;

interface RailConsoleProps {
    /** 日志行；为空时显示 waiting 占位行 */
    logs: RailLog[];
    /** 控制台占位文案（如"等待开始转换"） */
    waiting?: { title: string; detail?: string };
    /** 复制日志时的首行上下文（包名/任务号） */
    clipHeader?: string;
    className?: string;
}

export function RailConsole({ logs, waiting, clipHeader, className }: RailConsoleProps) {
    const t = useT();
    const logBoxRef = useRef<HTMLDivElement>(null);
    useLogFollow(logBoxRef);
    const clock = (logs[logs.length - 1]?.time ?? formatClock()).slice(0, 5);

    return (
        <section
            className={cn(
                "rounded-[12px] bg-[var(--console-bg)] backdrop-blur-[var(--blur-card)] shadow-[var(--shadow-card)] p-5 flex flex-col gap-[11px] w-full",
                className
            )}
        >
            <header className="flex items-center gap-2.5">
                <span className="text-[12px] leading-[16px] font-semibold text-text-1">
                    {t("lib.run-log", "运行日志")}
                </span>
                <span className="rounded-md bg-surface px-[7px] py-[2px] font-mono text-[10px] leading-[14px] text-text-3">
                    {clock}
                </span>
                <span className="flex-1" />
                <LogCopyButton
                    variant="floating"
                    logs={logs}
                    header={clipHeader}
                    className="h-6 w-6 rounded-md border-0 bg-transparent text-text-3 hover:bg-surface-2 hover:text-text-1"
                />
            </header>

            {/* 日志区：v5 没有内嵌小盒，行直接排在卡面上；定高滚动口径沿用原控制台
                （12.5vh 上限 100：1200×800 下正好 ≈4 行，窗口变矮跟着收） */}
            <div
                ref={logBoxRef}
                className="log-scroll h-[clamp(56px,12.5vh,100px)] overflow-y-auto flex flex-col gap-2"
            >
                {logs.length === 0 && waiting ? (
                    <p className="flex items-center gap-2.5 font-mono text-[11px] leading-[16px]">
                        <span className="font-semibold text-text-3">{waiting.title}</span>
                        {waiting.detail && (
                            <span className="text-text-3">{waiting.detail}</span>
                        )}
                    </p>
                ) : (
                    logs.map((l, i) => (
                        <p
                            key={i}
                            className="flex items-center gap-2.5 font-mono min-w-0 leading-[16px]"
                        >
                            <span className="text-[10.5px] text-text-3 shrink-0">
                                {l.time ?? formatClock()}
                            </span>
                            {l.stage && (
                                <span
                                    className={cn(
                                        "text-[10.5px] shrink-0",
                                        STAGE_COLOR[l.stage] ?? "text-amethyst"
                                    )}
                                >
                                    [{l.stage}]
                                </span>
                            )}
                            <span
                                className={cn(
                                    "text-[11px] truncate",
                                    LOG_LEVEL_COLOR[l.level ?? "info"]
                                )}
                            >
                                {tSource(l.message)}
                            </span>
                        </p>
                    ))
                )}
            </div>
        </section>
    );
}

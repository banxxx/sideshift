/**
 * 第 2 页：安装中
 *
 * 阶段清单只有三项（准备安装包 / 复制程序文件 / 建立数据目录）：快捷方式与卸载登记发生在
 * NSIS 内部，静默模式下壳既看不见也等不到，单列一项就只能靠猜填色——那是假进度。
 * 百分比同理，分母是安装目录里的真实字节数（见 Rust 侧 copy_pct）。
 *
 * 没有日志区：这一屏要回答的只有"还要等多久"和"出没出错"，中间过程逐条打印出来
 * 既读不懂也没法照做。失败就是把那句错误原因写在框里，成功就什么都不留。
 */
import { cn } from "@/lib/utils";
import type { Progress, Stage } from "./api";

const STAGES: Array<{ stage: Stage; label: string }> = [
    { stage: "prepare", label: "准备安装包" },
    { stage: "copy", label: "复制程序文件" },
    { stage: "data", label: "建立数据目录" },
];

type RowState = "idle" | "run" | "ok" | "err";

export function InstallPage({
    progress,
    error,
}: {
    progress: Progress | null;
    error: string | null;
}) {
    const idx = progress
        ? STAGES.findIndex((s) => s.stage === progress.stage)
        : -1;
    const done = progress?.done === true;
    const pct = done ? 100 : (progress?.pct ?? 0);
    const current = done
        ? "安装完成"
        : error
          ? "安装未完成"
          : `${STAGES[idx < 0 ? 0 : idx].label}…`;

    const stateOf = (i: number): RowState => {
        if (done) return "ok";
        if (i < idx) return "ok";
        if (i > idx) return "idle";
        return error ? "err" : "run";
    };

    return (
        <div className="page-in flex flex-col">
            <h1 className="text-[19px] leading-[26px] font-semibold text-text-1">
                正在安装
            </h1>
            <p className="mt-1 text-[12px] leading-[18px] text-text-2">
                窗口保持打开即可，安装过程不联网。
            </p>

            <div className="mt-4">
                <div className="flex items-baseline justify-between gap-3">
                    <span
                        aria-live="polite"
                        className="text-[13px] leading-[20px] font-medium text-text-1"
                    >
                        {current}
                    </span>
                    {!error && (
                        <span className="font-mono text-[12px] leading-[20px] text-text-2 tabular-nums">
                            {Math.round(pct)}%
                        </span>
                    )}
                </div>

                <div className="mt-2.5 h-1 overflow-hidden rounded-full bg-rail-track">
                    <span
                        className={cn(
                            "block h-full rounded-full transition-[width] duration-300 ease-out",
                            error ? "bg-redstone" : "bg-accent"
                        )}
                        style={{ width: `${pct}%` }}
                    />
                </div>

                <div className="mt-4 flex flex-col gap-3 border-t border-stroke-soft pt-3.5">
                    {STAGES.map((s, i) => (
                        <StageRow key={s.stage} label={s.label} state={stateOf(i)} />
                    ))}
                </div>

                {error && (
                    <div className="mt-[18px] rounded-md border border-stroke bg-bg-panel px-3 py-2.5">
                        <h3 className="text-[12px] leading-[18px] font-semibold text-redstone">
                            安装未完成
                        </h3>
                        <p className="mt-px text-[11px] leading-[16px] text-text-2">
                            {error}
                        </p>
                    </div>
                )}
            </div>
        </div>
    );
}

/** 阶段一行：左状态词右对齐，颜色是唯一的强调手段（无圆点图标、无脉冲） */
function StageRow({ label, state }: { label: string; state: RowState }) {
    return (
        <div className="flex items-baseline gap-3">
            <span
                className={cn(
                    "min-w-0 truncate text-[12px] leading-[18px]",
                    state === "idle"
                        ? "text-text-3"
                        : state === "run"
                          ? "font-medium text-text-1"
                          : "text-text-1"
                )}
            >
                {label}
            </span>
            <span
                className={cn(
                    "ml-auto shrink-0 text-[11px] leading-[16px]",
                    state === "ok" && "text-text-2",
                    state === "run" && "font-medium text-accent",
                    state === "err" && "text-redstone",
                    state === "idle" && "text-text-3"
                )}
            >
                {state === "ok"
                    ? "已完成"
                    : state === "run"
                      ? "进行中"
                      : state === "err"
                        ? "失败"
                        : "等待"}
            </span>
        </div>
    );
}

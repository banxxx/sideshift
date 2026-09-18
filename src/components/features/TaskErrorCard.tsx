/**
 * 任务错误卡（SS.pen Errors 族 `PXu3R`/`phjvm`，行5「错误状态族」规范）
 *
 * 设计原则：每个错误都必须带可操作出口，不能只报错。
 * 统一形态 = $surface 卡 + $stroke 1px + r12 + padding16 + 纵向 gap8（构建失败 gap10），
 * 头行「图标14 + 标题13/600 + 上下文（文件名/芯片/退出码）+ h28 内联按钮」，正文 12 $text-2。
 * 四类错误按出错阶段映射：
 *  - parser     解析失败    redstone X + 文件名 + 重新选择 / 查看解析日志
 *  - detector   依赖冲突    gold refresh-cw + 警告芯片 + 查看处置建议
 *  - downloader 下载失败    redstone download + 已重试 N 次 + 重试（accent）
 *  - builder    构建失败    redstone X + exit code + 日志尾块 + 复制诊断信息 / 重试构建
 */
import { Download, RefreshCw, X, type LucideIcon } from "lucide-react";
import type { TaskError } from "@/lib/types";
import { Btn, LinkBtn, ToneChip } from "@/components/design/ui";
import { cn } from "@/lib/utils";

interface TaskErrorCardProps {
    error: TaskError;
    /** 解析失败时展示的原始文件名 */
    fileName?: string;
    /** 重试（下载失败 / 构建失败） */
    onRetry?: () => void;
    /** 处置建议 / 重新选择：回到可修正错误的入口 */
    onFix?: () => void;
    /** 查看解析日志：滚动到日志控制台 */
    onShowLog?: () => void;
    /** 复制诊断信息（构建失败） */
    onCopyDiagnostics?: () => void;
    copied?: boolean;
}

/** 阶段 → 头行图标与色调 */
const STAGE_STYLE: Record<TaskError["stage"], { icon: LucideIcon; gold?: boolean }> = {
    parser: { icon: X },
    detector: { icon: RefreshCw, gold: true },
    downloader: { icon: Download },
    builder: { icon: X },
};

/** h28 内联按钮：描边态文字为 $text-1（设计稿 err 卡专用，比通用 outline 深一级） */
const INLINE_OUTLINE = "px-3 text-text-1";

export function TaskErrorCard({
    error,
    fileName,
    onRetry,
    onFix,
    onShowLog,
    onCopyDiagnostics,
    copied,
}: TaskErrorCardProps) {
    const { icon: Icon, gold } = STAGE_STYLE[error.stage];
    const builder = error.stage === "builder";

    return (
        <section
            className={cn(
                "flex w-full flex-col rounded-[12px] border border-stroke bg-surface p-4",
                builder ? "gap-2.5" : "gap-2"
            )}
        >
            {/* 头行：图标 + 标题 + 上下文 + 内联出口 */}
            <div className="flex w-full items-center gap-2">
                <Icon className={cn("size-3.5 shrink-0", gold ? "text-gold" : "text-redstone")} />
                <span className="shrink-0 text-[13px] leading-[20px] font-semibold text-text-1">
                    {error.title}
                </span>

                {error.stage === "parser" && fileName && (
                    <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        {fileName}
                    </span>
                )}
                {error.stage === "downloader" && error.attempts != null && error.attempts > 0 && (
                    <ToneChip tone="redstone" size="xs" className="bg-surface-2">
                        已重试 {error.attempts} 次
                    </ToneChip>
                )}
                {gold && (
                    <ToneChip tone="gold" size="xs" className="bg-surface-2">
                        警告
                    </ToneChip>
                )}
                {builder && error.exitCode != null && (
                    <span className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        exit code {error.exitCode}
                    </span>
                )}

                {!builder && onFix && (
                    <Btn size="xs" className={INLINE_OUTLINE} onClick={onFix}>
                        {error.stage === "parser" ? "重新选择" : "查看处置建议"}
                    </Btn>
                )}
                {!builder && error.retryable && onRetry && (
                    <Btn variant="primary" size="xs" className="px-3 font-semibold" onClick={onRetry}>
                        重试
                    </Btn>
                )}
            </div>

            {/* 详情 */}
            <p className="text-[12px] leading-[18px] font-normal text-text-2">{error.detail}</p>

            {/* 构建失败：日志尾块（等宽 10，末行 redstone） */}
            {builder && error.logTail && error.logTail.length > 0 && (
                <div className="flex w-full flex-col gap-1 rounded-lg bg-surface-2 p-3">
                    {error.logTail.map((line, i) => (
                        <span
                            key={i}
                            className={cn(
                                "truncate font-mono text-[10px] leading-[14px] font-normal",
                                i === error.logTail!.length - 1 ? "text-redstone" : "text-text-2"
                            )}
                        >
                            {line}
                        </span>
                    ))}
                </div>
            )}

            {/* 构建失败出口：两枚 h28 描边按钮 */}
            {builder && (
                <div className="flex w-full gap-2">
                    {onCopyDiagnostics && (
                        <Btn size="xs" className={INLINE_OUTLINE} onClick={onCopyDiagnostics}>
                            {copied ? "已复制" : "复制诊断信息"}
                        </Btn>
                    )}
                    {error.retryable && onRetry && (
                        <Btn size="xs" className={INLINE_OUTLINE} onClick={onRetry}>
                            重试构建
                        </Btn>
                    )}
                </div>
            )}

            {/* 解析失败：日志链接（11/600 accent） */}
            {onShowLog && (
                <LinkBtn size="sm" className="self-start" onClick={onShowLog}>
                    查看解析日志
                </LinkBtn>
            )}
        </section>
    );
}

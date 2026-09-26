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
 *  - installer  本机安装失败 redstone package-open + 重试（安装器没有退出码块，原因全在 detail 里）
 *  - builder    构建失败    redstone X + exit code + 日志尾块 + 复制诊断信息 / 重试构建
 */
import { Download, PackageOpen, RefreshCw, X, type LucideIcon } from "lucide-react";
import type { TaskError } from "@/lib/types";
import { truncateMiddle } from "@/lib/format";
import { tSource, useT } from "@/lib/i18n";
import { errOf } from "@/lib/errors";
import { Btn, LinkBtn, ToneChip } from "@/components/ui";
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
    installer: { icon: PackageOpen },
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
    const t = useT();
    const { icon: Icon, gold } = STAGE_STYLE[error.stage];
    const builder = error.stage === "builder";

    return (
        <section
            className={cn(
                "flex w-full flex-col rounded-[12px] border border-stroke bg-surface p-4",
                builder ? "gap-2.5" : "gap-2"
            )}
        >
                {/* 头行：图标 + 标题 + 上下文 + 内联出口。
                    error.title 是后端算好的固定句（动态键，就地查目录），下面这块登记
                    src-tauri 里会走到这张卡的那些：少一条，那条在英文界面就漏译。 */}
                <div className="flex w-full items-center gap-2">
                    <Icon className={cn("size-3.5 shrink-0", gold ? "text-gold" : "text-redstone")} />
                    {/*i18n:
                        解析失败
                        CurseForge 取链接失败
                        依赖解析失败
                        未选择加载器版本
                        服务端加载器获取失败
                        构建失败
                        本机没有可用的 Java
                        本机安装异常
                        安装目录准备失败
                        Java 起不来
                        本机安装超时
                        安装器报错
                        安装器报成功却没装出结果
                        本机安装已取消
                        文件获取失败
                        联网下载失败
                        取件失败
                        转换中断
                    */}
                    <span className="shrink-0 text-[13px] leading-[20px] font-semibold text-text-1">
                        {tSource(error.title)}
                    </span>

                {error.stage === "parser" && fileName && (
                    <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        {truncateMiddle(fileName, 28)}
                    </span>
                )}
                {error.stage === "downloader" && error.attempts != null && error.attempts > 0 && (
                    <ToneChip tone="redstone" size="xs" className="bg-surface-2">
                        {t("tasks.retried-count", "已重试 {{count}} 次", { count: error.attempts })}
                    </ToneChip>
                )}
                {gold && (
                    <ToneChip tone="gold" size="xs" className="bg-surface-2">
                        {t("tasks.warning", "警告")}
                    </ToneChip>
                )}
                {builder && error.exitCode != null && (
                    <span className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                        exit code {error.exitCode}
                    </span>
                )}

                {!builder && onFix && (
                    <Btn size="xs" className={INLINE_OUTLINE} onClick={onFix}>
                        {error.stage === "parser" ? t("common.re-select", "重新选择") : t("tasks.see-suggestions", "查看处置建议")}
                    </Btn>
                )}
                {!builder && error.retryable && onRetry && (
                    <Btn variant="primary" size="xs" className="px-3 font-semibold" onClick={onRetry}>
                        {t("tasks.retry", "重试")}
                    </Btn>
                )}
            </div>

            {/* 详情：网络类错误只说种类（后端给的 `code`），URL/状态码留在 `detail` 里给诊断信息；
                非网络的那批诊断串（带文件名/退出码/cause）查不中目录就照原样出中文 */}
            {/*i18n:
                找不到源整合包文件，请返回首页重新选择
                裸 zip 包无法自动确定 Loader 版本，请在转换配置中选择后重试
                应用退出时任务尚未完成，可重试
                未检测到 Java（本机没有可用的 JDK）
            */}
            <p className="text-[12px] leading-[18px] font-normal text-text-2">
                {errOf(error.code ?? error.detail)}
            </p>

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
                            {tSource(line)}
                        </span>
                    ))}
                </div>
            )}

            {/* 构建失败出口：两枚 h28 描边按钮 */}
            {builder && (
                <div className="flex w-full gap-2">
                    {onCopyDiagnostics && (
                        <Btn size="xs" className={INLINE_OUTLINE} onClick={onCopyDiagnostics}>
                            {copied ? t("common.copied", "已复制") : t("tasks.copy-diagnostics", "复制诊断信息")}
                        </Btn>
                    )}
                    {error.retryable && onRetry && (
                        <Btn size="xs" className={INLINE_OUTLINE} onClick={onRetry}>
                            {t("tasks.retry-build", "重试构建")}
                        </Btn>
                    )}
                </div>
            )}

            {/* 解析失败：日志链接（11/600 accent） */}
            {onShowLog && (
                <LinkBtn size="sm" className="self-start" onClick={onShowLog}>
                    {t("tasks.view-parse", "查看解析日志")}
                </LinkBtn>
            )}
        </section>
    );
}

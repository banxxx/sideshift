/**
 * 崩溃面板：渲染期整棵树崩掉、或挂树之前就失败时换掉窗口本体，不再白屏。
 * 必须零依赖应用组件（提示区、按钮本身也可能在崩溃的那棵树里），所以这里只用最基础的令牌。
 */
import { Component, type ErrorInfo, type ReactNode } from "react";
import { t } from "@/lib/i18n";

interface Crash {
    /** null = 没崩过；有值就是那句要显示的引擎错误 */
    detail: string | null;
}

export function CrashPanel({ detail }: { detail: string }) {
    return (
        <div className="relative h-screen p-[var(--win-inset)]">
            <div className="h-full flex flex-col overflow-hidden bg-background text-foreground shadow-[var(--win-shadow)]">
                <div className="flex-1 flex flex-col items-center justify-center gap-2 px-8 text-center">
                    <p className="font-heading text-[15px] leading-[22px] font-semibold text-text-1">
                        {t("shell.crash-title", "界面出错了")}
                    </p>
                    <p className="text-[12px] leading-[18px] font-normal text-text-2">
                        {t("shell.crash-sub", "重新载入即可继续，已排队的转换不受影响")}
                    </p>
                    <p className="mt-1 max-w-[520px] font-mono text-[11px] leading-[16px] font-normal break-all text-text-3">
                        {detail}
                    </p>
                    <button
                        type="button"
                        className="mt-3 rounded-md border border-stroke bg-surface px-4 py-2 text-[12px] font-medium text-text-1 hover:bg-surface-2"
                        onClick={() => window.location.reload()}
                    >
                        {t("shell.crash-reload", "重新载入")}
                    </button>
                </div>
            </div>
        </div>
    );
}

export class ErrorBoundary extends Component<{ children: ReactNode }, Crash> {
    state: Crash = { detail: null };

    static getDerivedStateFromError(e: unknown): Crash {
        return { detail: e instanceof Error ? `${e.name}: ${e.message}` : String(e) };
    }

    componentDidCatch(error: Error, info: ErrorInfo): void {
        console.error("[render]", error, info.componentStack);
    }

    render(): ReactNode {
        if (this.state.detail === null) return this.props.children;
        return <CrashPanel detail={this.state.detail} />;
    }
}

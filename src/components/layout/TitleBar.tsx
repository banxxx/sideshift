import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, Copy } from "lucide-react";
import { cn } from "@/lib/utils";

const appWindow = getCurrentWindow();

export function TitleBar() {
    const [isMaximized, setIsMaximized] = useState(false);

    // 监听最大化状态变化
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        appWindow.isMaximized().then(setIsMaximized);
        appWindow.onResized(() => {
            appWindow.isMaximized().then(setIsMaximized);
        }).then((fn) => {
            unlisten = fn;
        });
        return () => {
            unlisten?.();
        };
    }, []);

    return (
        <header
            data-tauri-drag-region
            className="h-10 shrink-0 flex items-center select-none bg-card border-b"
        >
            {/* 左侧：Logo + 应用名 */}
            <div
                data-tauri-drag-region
                className="flex items-center gap-2 pl-4 pr-6 h-full"
            >
                <div className="w-5 h-5 rounded bg-primary flex items-center justify-center">
          <span className="text-[10px] font-bold text-primary-foreground">
            S
          </span>
                </div>
                <span className="text-sm font-medium tracking-tight">SideShift</span>
            </div>

            {/* 中间：拖拽区域，双击最大化 */}
            <div
                data-tauri-drag-region
                className="flex-1 h-full"
                onDoubleClick={() => appWindow.toggleMaximize()}
            />

            {/* 右侧：窗口控制按钮 */}
            <div className="flex items-center h-full">
                <WindowButton
                    onClick={() => appWindow.minimize()}
                    title="最小化"
                >
                    <Minus className="h-3.5 w-3.5" />
                </WindowButton>

                <WindowButton
                    onClick={() => appWindow.toggleMaximize()}
                    title={isMaximized ? "还原" : "最大化"}
                >
                    {isMaximized ? (
                        <Copy className="h-3 w-3" />
                    ) : (
                        <Square className="h-3 w-3" />
                    )}
                </WindowButton>

                <WindowButton
                    onClick={() => appWindow.close()}
                    title="关闭"
                    variant="close"
                >
                    <X className="h-4 w-4" />
                </WindowButton>
            </div>
        </header>
    );
}

interface WindowButtonProps {
    onClick: () => void;
    title: string;
    variant?: "default" | "close";
    children: React.ReactNode;
}

function WindowButton({
                          onClick,
                          title,
                          variant = "default",
                          children,
                      }: WindowButtonProps) {
    return (
        <button
            onClick={onClick}
            title={title}
            className={cn(
                "h-full w-12 flex items-center justify-center transition-colors",
                variant === "close"
                    ? "hover:bg-red-500 hover:text-white"
                    : "hover:bg-accent text-muted-foreground hover:text-foreground"
            )}
        >
            {children}
        </button>
    );
}
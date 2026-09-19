/**
 * 自定义窗口标题栏（Tauri decorations:false，对应 SS.pen 各帧 TitleBar）
 *
 * 布局（设计稿 TitleBar）：h40 / $bg-panel / 下描边 $stroke-soft / padding 左右 16 / gap 10
 * 内容：LogoMark 16×16 无圆角 | 拖拽区（双击最大化） | 窗口控件
 * 窗口控件规格（v17 定稿）：gap 2、34×26、圆角 6、ghost 无底色、图标 13px $text-2；
 * 处于二级页面时（canGoBack），最小化左侧出现 undo-2 返回按钮，
 * 并用 1×14 短竖线（$stroke）与最小化隔开。窗口控制逻辑见 @/lib/window-controls。
 */
import { Minus, Square, X, Copy, Undo2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useNavigation } from "@/lib/navigation";
import { useWindowControls } from "@/lib/window-controls";

export function TitleBar() {
    const { canGoBack, back } = useNavigation();
    const win = useWindowControls();

    return (
        <header
            data-tauri-drag-region
            className="h-10 shrink-0 flex items-center select-none bg-bg-panel border-b border-stroke-soft px-4"
        >
            {/* 左侧：仅 Logo（设计稿定稿：logo 右边不放文字） */}
            <div
                data-tauri-drag-region
                className="flex items-center h-full"
            >
                <div className="size-4 flex flex-col">
                    {/* 草方块信标：上草下泥，与 public/logo.svg 同源（无圆角，展示原始方块） */}
                    <div className="h-[5px] shrink-0 bg-grass-top" />
                    <div className="flex-1 bg-accent" />
                </div>
            </div>

            {/* 中间：拖拽区域，双击最大化 */}
            <div
                data-tauri-drag-region
                className="flex-1 h-full"
                onDoubleClick={win.toggleMaximize}
            />

            {/* 右侧：窗口控件（gap 2px；二级页多出一个返回按钮 + 分隔竖线） */}
            <div className="flex items-center gap-0.5 h-full">
                {canGoBack && (
                    <>
                        <WindowButton onClick={back} title="返回上一页">
                            <Undo2 className="size-[13px]" />
                        </WindowButton>
                        <span
                            aria-hidden
                            className="h-3.5 w-px bg-stroke self-center"
                        />
                    </>
                )}
                <WindowButton onClick={win.minimize} title="最小化">
                    <Minus className="size-[13px]" />
                </WindowButton>
                <WindowButton
                    onClick={win.toggleMaximize}
                    title={win.isMaximized ? "还原" : "最大化"}
                >
                    {win.isMaximized ? (
                        <Copy className="size-3" />
                    ) : (
                        <Square className="size-3" />
                    )}
                </WindowButton>
                <WindowButton
                    onClick={win.close}
                    title="关闭"
                    variant="close"
                >
                    <X className="size-[13px]" />
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

/** 单个窗口控件：34×26 r6 ghost；关闭按钮 hover 红石色（设计稿规范） */
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
                "h-[26px] w-[34px] rounded-md flex items-center justify-center transition-colors",
                "text-text-2 hover:bg-surface-2 hover:text-text-1",
                variant === "close" &&
                "hover:bg-redstone-dim hover:text-redstone"
            )}
        >
            {children}
        </button>
    );
}

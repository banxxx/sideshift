/**
 * 自定义窗口标题栏（Tauri decorations:false）：h40，Logo | 拖拽区（双击最大化）| 窗口控件（34×26、ghost 无底色、图标 13px）。
 * 二级页时（canGoBack）最小化左侧出现返回按钮，用 1×14 短竖线与最小化隔开；窗口控制逻辑见 @/lib/window-controls。
 */
import { Minus, Square, X, Copy, Undo2 } from "lucide-react";
import { Collapse, Tip, HOVER_PRESS, Logo } from "@/components/ui";
import { SWAP_IN } from "@/lib/page-motion";
import { cn } from "@/lib/utils";
import { useNavigation } from "@/lib/navigation";
import { useWindowControls } from "@/lib/window-controls";
import { useT } from "@/lib/i18n";

export function TitleBar() {
    const { canGoBack, back } = useNavigation();
    const win = useWindowControls();
    const t = useT();

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
                <Logo />
            </div>

            {/* 中间：拖拽区域，双击最大化 */}
            <div
                data-tauri-drag-region
                className="flex-1 h-full"
                onDoubleClick={win.toggleMaximize}
            />

            {/* 右侧：窗口控件（gap 2px；二级页多出一个返回按钮 + 分隔竖线）。
                返回件走 `Collapse axis="x"`（横向收放一格），**不走 `popLayout`**：popLayout 把退场层
                按 `offsetParent` 的 left 钉住（motion 13.4 `PopChild.mjs`：`position:absolute !important` +
                `left: offsetLeft`），而这一组是右对齐的——抽走一格让容器自身左边缘当场右移 39px，
                钉在"容器内 left:0"上的返回件就跟着平移 39px，正好压在「最小化」上淡出 ⇒ 他报的「闪一下 +
                位置不对」。而且那三个控件是原生 button，popLayout 只带 motion 节点的位移动画，它们当场硬跳。
                改成在流内收自己的宽度：整组一起滑，谁也不跳。`gap={2}` 收掉自己占的那格格距（Collapse 第 2 条）。 */}
            <div className="flex items-center gap-0.5 h-full">
                <Collapse
                    when={canGoBack}
                    gap={2}
                    axis="x"
                    transition={SWAP_IN}
                    className="flex items-center gap-0.5"
                >
                    <WindowButton onClick={back} title={t("shell.back", "返回上一页")}>
                        <Undo2 className="size-[13px]" />
                    </WindowButton>
                    <span aria-hidden className="h-3.5 w-px shrink-0 self-center bg-stroke" />
                </Collapse>
                <WindowButton onClick={win.minimize} title={t("shell.minimize", "最小化")}>
                    <Minus className="size-[13px]" />
                </WindowButton>
                <WindowButton
                    onClick={win.toggleMaximize}
                    title={win.isMaximized ? t("shell.restore", "还原") : t("shell.maximize", "最大化")}
                >
                    {win.isMaximized ? (
                        <Copy className="size-3" />
                    ) : (
                        <Square className="size-3" />
                    )}
                </WindowButton>
                <WindowButton
                    onClick={win.close}
                    title={t("common.close", "关闭")}
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

/**
 * 单个窗口控件：34×26 r6 ghost；关闭按钮 hover 红石色（设计稿规范）。
 * `shrink-0`：返回件那格横向收放时，壳里的控件不能被 flex 压扁，得保持 34px 让壳去裁（Collapse 第 3 条）。
 * title 不落 DOM（系统灰泡不受样式管）：补 aria-label 保住无障碍名，气泡交给 Tip
 */
function WindowButton({
                          onClick,
                          title,
                          variant = "default",
                          children,
                      }: WindowButtonProps) {
    return (
        <button
            onClick={onClick}
            aria-label={title}
            className={cn(
                "group/tip relative flex h-[26px] w-[34px] shrink-0 items-center justify-center rounded-md",
                "text-text-2 hover:bg-surface-2 hover:text-text-1",
                HOVER_PRESS,
                variant === "close" &&
                "hover:bg-redstone-dim hover:text-redstone"
            )}
        >
            {children}
            <Tip label={title} />
        </button>
    );
}

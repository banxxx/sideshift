/**
 * 安装壳标题栏：与主应用 TitleBar 同一套几何（h40 / $bg-panel / 下描边 $stroke-soft / padding 16）
 *
 * 窗口控件只留关闭：窗口不可调尺寸，最大化是把界面拉坏的入口；最小化在这条三页流程里没有
 * 去处（进度在 Rust 的 blocking 线程里跑，缩到任务栏反而更容易让人以为已经装完了）。
 * 主题也不给开关——深浅由系统偏好决定，安装器不该比装的东西多一个用户要做的决定。
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { X } from "lucide-react";
import { Tip, TIP_TRIGGER } from "@/components/ui/Tip";
import { Logo } from "@/components/ui/Logo";
import { cn } from "@/lib/utils";

export function TitleBar() {
    return (
        <header
            data-tauri-drag-region
            className="flex h-10 shrink-0 select-none items-center gap-2.5 border-b border-stroke-soft bg-bg-panel px-4"
        >
            <Logo />
            <span className="text-[12px] leading-[18px] font-medium text-text-2">
                SideShift 安装程序
            </span>
            <button
                onClick={() => void getCurrentWindow().close()}
                aria-label="关闭"
                className={cn(
                    TIP_TRIGGER,
                    "ml-auto flex h-[26px] w-[34px] items-center justify-center rounded-md",
                    "text-text-2 transition-colors hover:bg-redstone-dim hover:text-redstone"
                )}
            >
                <X className="size-[13px]" />
                <Tip label="关闭" />
            </button>
        </header>
    );
}

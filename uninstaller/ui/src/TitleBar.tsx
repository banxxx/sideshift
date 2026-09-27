/**
 * 卸载壳标题栏：几何与安装壳同一套（h40 / $bg-panel / 下描边 $stroke-soft / padding 16）
 *
 * 版本 + 架构放在这里而不是正文：卸载现场没人会为了确认版本去翻设置，
 * 但「装错了包」这一怀疑只有在这里能自查（安装壳把它放在流程条右侧，是同一个道理）。
 *
 * 只有关闭：窗口不可调尺寸，最小化在一个正在删文件的窗口上没有意义。
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { X } from "lucide-react";
import { Tip, TIP_TRIGGER } from "@/components/ui/Tip";
import { Logo } from "@/components/ui/Logo";
import { cn } from "@/lib/utils";

export function TitleBar({ meta }: { meta: string | null }) {
    return (
        <header
            data-tauri-drag-region
            className="flex h-10 shrink-0 select-none items-center gap-2.5 border-b border-stroke-soft bg-bg-panel px-4"
        >
            <Logo />
            <span className="text-[12px] leading-[18px] font-medium text-text-2">
                SideShift 卸载程序
            </span>
            <span className="ml-auto font-mono text-[11px] leading-[18px] text-text-3 tabular-nums">
                {meta ?? "正在读取本机…"}
            </span>
            <button
                onClick={() => void getCurrentWindow().close()}
                aria-label="关闭"
                className={cn(
                    TIP_TRIGGER,
                    "flex h-[26px] w-[34px] items-center justify-center rounded-md",
                    "text-text-2 transition-colors hover:bg-redstone-dim hover:text-redstone"
                )}
            >
                <X className="size-[13px]" />
                <Tip label="关闭" />
            </button>
        </header>
    );
}

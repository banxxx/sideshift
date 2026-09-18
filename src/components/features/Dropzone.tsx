/**
 * 整合包拖放卡（SS.pen Home·Idle `k0nEJ` / Home `pqC6C`）
 *
 * Idle 大卡（760×360，含帮助说明行）与选包后紧凑卡（540 宽）共用本组件，
 * 仅尺寸与附加文案不同。拖放说明：
 * - Tauri 环境下 OS 文件拖入不会走 HTML5 drop 事件，由 @tauri-apps/api/webview
 *   的 onDragDropEvent 回调把文件路径交给 onDropPaths（见 HomePage）；
 *   本组件内的 onDragOver 只负责高亮反馈，浏览器 dev 下 drop 退化为读取文件名
 *   交给 mock 解析，保证脱离后端也能走通全流程。
 * - 点击整卡 = 打开系统文件选择框（onPick）。
 */
import { useState } from "react";
import { Upload } from "lucide-react";
import { cn } from "@/lib/utils";
import { isTauri } from "@/lib/api";

interface DropzoneProps {
    onPick: () => void;
    /** 浏览器兜底：从 HTML5 DataTransfer 取路径（Tauri 下由 webview 事件替代） */
    onDropPaths?: (paths: string[]) => void;
    /** 紧凑模式（选包后 540 宽）下不显示"识别后显示…"提示行 */
    compact?: boolean;
    busy?: boolean;
    className?: string;
}

export function Dropzone({ onPick, onDropPaths, compact, busy, className }: DropzoneProps) {
    const [dragging, setDragging] = useState(false);

    return (
        <div
            role="button"
            tabIndex={0}
            onClick={onPick}
            onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onPick()}
            onDragOver={(e) => {
                e.preventDefault();
                setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(e) => {
                e.preventDefault();
                setDragging(false);
                // Tauri 下真实路径由 webview onDragDropEvent 下发（见 useTauriFileDrop），
                // HTML5 drop 拿不到路径；浏览器 dev 没有路径概念，退而用文件名驱动 mock 解析
                const native = (e.nativeEvent as DragEvent & { paths?: string[] })
                    .paths;
                const list =
                    native && native.length > 0
                        ? native
                        : isTauri
                          ? []
                          : Array.from(e.dataTransfer.files).map((f) => f.name);
                if (list.length > 0) onDropPaths?.(list);
            }}
            className={cn(
                "bg-surface border rounded-[12px] px-7 py-8 flex flex-col items-center justify-center gap-4 cursor-pointer select-none transition-colors",
                dragging ? "border-accent" : "border-stroke",
                compact ? "w-[540px] shrink-0" : "w-[760px] h-[360px]",
                busy && "pointer-events-none opacity-70",
                className
            )}
        >
            {/* 上传图标盒：52×52 r12 emerald-dim + emerald upload */}
            <span className="size-[52px] rounded-[12px] bg-emerald-dim flex items-center justify-center">
                <Upload className="size-[22px] text-emerald" />
            </span>

            <div className="flex flex-col items-center gap-1.5">
                <p className="text-[15px] font-semibold text-text-1">拖入客户端整合包</p>
                <p className="font-mono text-xs text-text-3">支持 .mrpack · .zip · .7z</p>
                {!compact && (
                    <p className="text-xs text-text-3">
                        识别后显示加载器、Minecraft 版本、模组数量与包体积
                    </p>
                )}
                <p className="text-[13px] font-semibold text-accent">
                    或点击选择文件
                </p>
            </div>
        </div>
    );
}

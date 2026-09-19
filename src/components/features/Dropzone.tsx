/**
 * 整合包拖放卡（SS.pen Home·Idle `k0nEJ` / Home `pqC6C`）
 *
 * Idle 大卡（760×360，含帮助说明行）与选包后紧凑卡（540 宽）共用本组件。
 * 动画全部由 motion 驱动（App.css 只保留 --dz-glow 主题色与 .dz-ants-rect 尺寸）：
 * - 常态：$surface 底 + $stroke 描边；
 * - 激活（悬停或拖拽同一套效果）：描边透明、SVG 蚂蚁线圆角跑动边框接管四边，
 *   弹簧上浮 + accent 柔光投影 + 淡底蒙层 + 涟漪脉冲 + 图标反色浮动；
 *   悬停时另有一层跟随光标的径向光斑；仅主文案区分（拖拽→"松开，交给 SideShift"，
 *   文案切换用 AnimatePresence 做上下滑退场/入场）。
 * 拖放通道说明：
 * - Tauri 下 OS 文件拖入不走 DOM drag 事件，悬停高亮由 useTauriFileDrop 的
 *   enter/over/leave 标志经 fileDragging prop 传入，真实路径也由它下发；
 *   浏览器 dev 下 dragover/dragleave 生效，drop 退化为读取文件名交给 mock 解析。
 * - 点击整卡 = 打开系统文件选择框（onPick）。
 */
import { useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Upload } from "lucide-react";
import { cn } from "@/lib/utils";
import { isTauri } from "@/lib/api";

interface DropzoneProps {
    onPick: () => void;
    /** 浏览器兜底：从 HTML5 DataTransfer 取路径（Tauri 下由 webview 事件替代） */
    onDropPaths?: (paths: string[]) => void;
    /** Tauri：OS 文件正在拖拽悬停于窗口（webview enter/over/leave 事件驱动） */
    fileDragging?: boolean;
    /** 紧凑模式（选包后 540 宽）下不显示"识别后显示…"提示行 */
    compact?: boolean;
    busy?: boolean;
    className?: string;
}

/** 柔光/边框等动画由 motion 驱动；此处仅两个共享节奏常量 */
const FADE = { duration: 0.2 } as const;
/** 上浮用弹簧，比线性位移更像"被托起来" */
const LIFT_SPRING = { type: "spring", stiffness: 380, damping: 28 } as const;

export function Dropzone({
    onPick,
    onDropPaths,
    fileDragging,
    compact,
    busy,
    className,
}: DropzoneProps) {
    const [hovering, setHovering] = useState(false);
    const [dragOver, setDragOver] = useState(false);
    const dragging = dragOver || !!fileDragging;
    // 悬停与拖拽共用同一套激活效果；仅文案区分语义
    const active = hovering || dragging;

    return (
        <motion.div
            role="button"
            tabIndex={0}
            onClick={onPick}
            onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onPick()}
            onMouseEnter={() => setHovering(true)}
            onMouseLeave={() => setHovering(false)}
            onFocus={() => setHovering(true)}
            onBlur={() => setHovering(false)}
            onMouseMove={(e) => {
                // 光标光斑位置写进 CSS 变量，避免 mousemove 触发 React 重渲染
                const el = e.currentTarget;
                const r = el.getBoundingClientRect();
                el.style.setProperty("--dz-x", `${e.clientX - r.left}px`);
                el.style.setProperty("--dz-y", `${e.clientY - r.top}px`);
            }}
            onDragOver={(e) => {
                e.preventDefault();
                setDragOver(true);
            }}
            onDragLeave={() => setDragOver(false)}
            onDrop={(e) => {
                e.preventDefault();
                setDragOver(false);
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
            initial={false}
            animate={{ y: active ? -2 : 0 }}
            transition={LIFT_SPRING}
            className={cn(
                "group relative rounded-[12px] bg-surface border px-7 py-8 flex flex-col items-center justify-center gap-4 cursor-pointer select-none transition-colors duration-200",
                active ? "border-transparent" : "border-stroke",
                compact ? "w-[540px] shrink-0" : "w-[760px] h-[360px]",
                busy && "pointer-events-none opacity-70",
                className
            )}
        >
            {/* 柔光投影层：阴影色由 CSS 给出（--dz-glow 随主题），motion 只淡入淡出 */}
            <motion.span
                aria-hidden
                className="pointer-events-none absolute inset-0 rounded-[12px] shadow-[0_18px_40px_-18px_var(--dz-glow)]"
                initial={false}
                animate={{ opacity: active ? 1 : 0 }}
                transition={FADE}
            />
            {/* 跟随光标的径向光斑（悬停期有效；OS 拖拽时没有鼠标位置） */}
            <motion.span
                aria-hidden
                className="pointer-events-none absolute inset-0 rounded-[12px]"
                style={{
                    background:
                        "radial-gradient(260px circle at var(--dz-x, 50%) var(--dz-y, 50%), var(--accent-dim), transparent 70%)",
                }}
                initial={false}
                animate={{ opacity: active && !dragging ? 1 : 0 }}
                transition={{ duration: 0.3 }}
            />
            {/* 激活淡底蒙层 */}
            <motion.span
                aria-hidden
                className="pointer-events-none absolute inset-0 rounded-[12px] bg-accent-dim/60"
                initial={false}
                animate={{ opacity: active ? 1 : 0 }}
                transition={FADE}
            />
            {/* 蚂蚁线跑动边框（替代描边）：SVG 沿圆角矩形的描边路径，
                stroke-dashoffset 由 motion 循环位移，虚线会顺着圆弧"走"过四角 */}
            <motion.svg
                aria-hidden
                className="pointer-events-none absolute inset-0 h-full w-full"
                initial={false}
                animate={{ opacity: active ? 1 : 0 }}
                transition={FADE}
            >
                <motion.rect
                    className="dz-ants-rect"
                    x={1}
                    y={1}
                    rx={11}
                    fill="none"
                    style={{ stroke: "var(--accent)" }}
                    strokeWidth={2}
                    strokeDasharray="12 12"
                    initial={false}
                    animate={
                        active
                            ? { strokeDashoffset: [0, -24] }
                            : { strokeDashoffset: 0 }
                    }
                    transition={{
                        strokeDashoffset: {
                            duration: 0.9,
                            ease: "linear",
                            repeat: Infinity,
                        },
                    }}
                />
            </motion.svg>
            {/* 涟漪脉冲环：只在激活期间挂载，放大同时淡出、无限循环 */}
            <AnimatePresence>
                {active && (
                    <motion.span
                        key="dz-pulse"
                        aria-hidden
                        className="pointer-events-none absolute inset-[2px] rounded-[10px] border-2 border-accent/40"
                        initial={{ scale: 1, opacity: 0.6 }}
                        animate={{ scale: 1.008, opacity: 0 }}
                        exit={{ opacity: 0, transition: { duration: 0.15 } }}
                        transition={{
                            duration: 1.6,
                            ease: "easeOut",
                            repeat: Infinity,
                        }}
                    />
                )}
            </AnimatePresence>

            {/* 上传图标盒：52×52 r12；激活时 accent 实心 + 缓慢浮动 */}
            <motion.span
                className={cn(
                    "relative flex size-[52px] items-center justify-center rounded-[12px] transition-colors duration-200",
                    active ? "bg-accent text-accent-ink" : "bg-emerald-dim text-emerald"
                )}
                initial={false}
                animate={active ? { y: [0, -4, 0] } : { y: 0 }}
                transition={
                    active
                        ? { y: { duration: 1.4, ease: "easeInOut", repeat: Infinity } }
                        : { duration: 0.2 }
                }
            >
                <Upload className="size-[22px]" />
            </motion.span>

            <div className="relative flex flex-col items-center gap-1.5">
                {/* 主文案定高，切换时旧字上滑退场、新字下滑入场 */}
                <div className="flex h-[22px] items-center justify-center">
                    <AnimatePresence mode="wait" initial={false}>
                        <motion.p
                            key={dragging ? "drag" : "idle"}
                            className={cn(
                                "text-[15px] font-semibold transition-colors duration-200",
                                active ? "text-accent" : "text-text-1"
                            )}
                            initial={{ opacity: 0, y: 8 }}
                            animate={{ opacity: 1, y: 0 }}
                            exit={{ opacity: 0, y: -8 }}
                            transition={{ duration: 0.15 }}
                        >
                            {dragging ? "松开，交给 SideShift" : "拖入客户端整合包"}
                        </motion.p>
                    </AnimatePresence>
                </div>
                <p className="font-mono text-xs text-text-3">
                    支持 .mrpack · .zip · .7z
                </p>
                {!compact && (
                    <p className="text-xs text-text-3">
                        识别后显示加载器、Minecraft 版本、模组数量与包体积
                    </p>
                )}
                <p className="text-[13px] font-semibold text-accent">
                    或点击选择文件
                </p>
            </div>
        </motion.div>
    );
}

/**
 * 整合包拖放卡（SS.pen Home·Idle `k0nEJ` / Home `pqC6C`）
 *
 * Idle 大卡（760×360，含帮助说明行）与选包后紧凑卡共用本组件。紧凑卡随窗口流体：
 * 宽度挂在 4px 悬停壳上（壳是行内 flex 项，百分比才有确定包含块），卡片 w-full 铺满，
 * 1200 窗口下即设计稿的 540。
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
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Upload } from "lucide-react";
import { cn } from "@/lib/utils";
import { isTauri } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { LIFT, MORPH } from "@/lib/springs";

interface DropzoneProps {
    /** 打开文件框；可返回 Promise（选包对话框），settle 后组件会复位悬停态 */
    onPick: () => unknown;
    /** 浏览器兜底：从 HTML5 DataTransfer 取路径（Tauri 下由 webview 事件替代） */
    onDropPaths?: (paths: string[]) => void;
    /** Tauri：OS 文件正在拖拽悬停于窗口（webview enter/over/leave 事件驱动） */
    fileDragging?: boolean;
    /** motion 共享元素变换 id：Home 大卡↔紧凑卡时自动做"缩小+归位"morph */
    layoutId?: string;
    /** 紧凑模式（选包后 540 宽）下不显示"识别后显示…"提示行 */
    compact?: boolean;
    busy?: boolean;
    className?: string;
}

/** 柔光/边框等动画由 motion 驱动；此处仅一个共享节奏常量（两根弹簧在 @/lib/springs） */
const FADE = { duration: 0.2 } as const;

export function Dropzone({
    onPick,
    onDropPaths,
    fileDragging,
    layoutId,
    compact,
    busy,
    className,
}: DropzoneProps) {
    const t = useT();
    const [hovering, setHovering] = useState(false);
    const [dragOver, setDragOver] = useState(false);
    const dragging = dragOver || !!fileDragging;
    // 悬停与拖拽共用同一套激活效果；仅文案区分语义
    const active = hovering || dragging;

    // 蚂蚁线无缝化：虚线周期 24，但矩形周长一般不是 24 的整数倍，
    // 首尾（左上角起点处）相位对不齐会露出接缝。测出真实周长后把
    // pathLength 设成最近的 24 整数倍，浏览器按比例重标定虚线单位，首尾必然闭合。
    const antsRef = useRef<SVGRectElement | null>(null);
    const [antsPathLength, setAntsPathLength] = useState<number>();
    useLayoutEffect(() => {
        const el = antsRef.current;
        if (!el) return;
        const fit = () => {
            const cycles = Math.max(1, Math.round(el.getTotalLength() / 24));
            setAntsPathLength(cycles * 24);
        };
        fit();
        const ro = new ResizeObserver(fit);
        ro.observe(el);
        return () => ro.disconnect();
    }, []);

    // 兜底：Alt+Tab 切走再回来等同步信号丢失场景，窗口焦点变化时复位悬停态。
    // （对模态文件框不可依赖：WebView2 下原生对话框开关不一定派发 window blur）
    useEffect(() => {
        const rearm = () => {
            setHovering(false);
            setDragOver(false);
        };
        window.addEventListener("blur", rearm);
        window.addEventListener("focus", rearm);
        return () => {
            window.removeEventListener("blur", rearm);
            window.removeEventListener("focus", rearm);
        };
    }, []);

    // 模态文件框打开期间主窗口被禁用，Chromium 直接丢失这段时间的光标跟踪：
    // 关闭后不会给"曾悬停"的卡片补发任何 mouseout/mouseleave（React 合成
    // leave 依赖它们），hovering 便永远卡住。唯一确定性的关闭信号是 onPick
    // 返回的 Promise settle（选中与取消两条路都会走到），在此手动复位。
    const pickAndRearm = () => {
        if (busy) return;
        void (async () => {
            try {
                await onPick();
            } finally {
                setHovering(false);
                setDragOver(false);
            }
        })();
    };

    return (
        // 静态悬停壳：卡片本体激活时上浮 2px，若 enter/leave 挂在会动的本体上，
        // 边缘光标点会被位移甩进甩出形成高频闪烁（扩热区也无效——热区跟着一起动）。
        // 这层壳几何永不移位，padding 4px > 位移 2px，构成真正的滞回带。
        <div
            className={cn(
                "-m-[4px] p-[4px]",
                // 紧凑态的流体宽度写在这层壳上：壳是行内的 flex 项，百分比相对行宽解析
                // 才有效；若把百分比写在卡片上，壳会先收缩到内容宽 → 百分比按 auto 解析、
                // 整张卡塌成文字宽度。idle 保持设计稿的 760 定宽（最小窗口内容区 844，
                // 永远放得下），不用 self-stretch 撑满——那会把整行变成悬停热区。
                compact && "shrink-0 w-[clamp(428px,59.3%,648px)]"
            )}
            onMouseEnter={() => setHovering(true)}
            onMouseLeave={() => setHovering(false)}
        >
            <motion.div
                role="button"
                tabIndex={0}
                // busy 时不能点选，但绝不能用 pointer-events-none 屏蔽：那会让解析期间的
                // mouseleave 丢失，指针移开后 hovering 卡死、蚂蚁线常亮（守卫在 pickAndRearm 里做）
                onClick={pickAndRearm}
                onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") pickAndRearm();
                }}
                onFocus={(e) => {
                    // 仅键盘 Tab 聚焦（:focus-visible）才点亮；鼠标点击留下的常驻焦点
                    // 不是悬停——否则对话框关闭后焦点回归卡片，激活态又被点亮
                    if (e.target.matches(":focus-visible")) setHovering(true);
                }}
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
                    const native = (e.nativeEvent as DragEvent & {
                        paths?: string[];
                    }).paths;
                    const list =
                        native && native.length > 0
                            ? native
                            : isTauri
                              ? []
                              : Array.from(e.dataTransfer.files).map(
                                    (f) => f.name
                                );
                    if (list.length > 0) onDropPaths?.(list);
                }}
                initial={false}
                layoutId={layoutId}
                animate={{ y: active ? -2 : 0 }}
                transition={{ ...LIFT, layout: MORPH }}
                className={cn(
                    "group relative rounded-[12px] bg-surface border px-7 py-8 flex flex-col items-center justify-center gap-4 cursor-pointer select-none transition-[background-color,color,border-color,opacity] duration-200",
                    active ? "border-transparent" : "border-stroke",
                    // 紧凑态：卡片铺满壳（宽度由壳给出）
                    // idle 态：设计稿 760 定宽 + 高度按视口收（45vh 在 800 高窗口正好 360）
                    compact
                        ? "h-full w-full"
                        : "w-[760px] h-[clamp(288px,45vh,360px)]",
                    busy && "opacity-70",
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
                        ref={antsRef}
                        className="dz-ants-rect"
                        x={1}
                        y={1}
                        rx={11}
                        fill="none"
                        pathLength={antsPathLength}
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

                {/* 上传图标盒：52×52 r12；激活时 accent 实心 + 缓慢浮动。
                    静置用 accent-dim/accent 同族淡底：这张卡的颜色语言全程是品牌蓝
                    （蚂蚁线、柔光、光斑、淡底、"或点击选择文件"都是 $accent），绿色在
                    这里既抢戏又错语义——$emerald 全站只表「已完成 / 已保留 / 服务端」，
                    还没选包的那一步不属于任何一项。 */}
                <motion.span
                    className={cn(
                        "relative flex size-[52px] items-center justify-center rounded-[12px] transition-colors duration-200",
                        active ? "bg-accent text-accent-ink" : "bg-accent-dim text-accent"
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
                                {dragging ? t("home.release-hand", "松开，交给 SideShift") : t("home.drop-client", "拖入客户端整合包")}
                            </motion.p>
                        </AnimatePresence>
                    </div>
                    <p className="font-mono text-xs text-text-3">
                        {t("home.supports-mrpack", "支持 .mrpack · .zip 文件格式")}
                    </p>
                    <p className="text-[13px] font-semibold text-accent">
                        {t("home.click-browse", "或点击选择文件")}
                    </p>
                </div>
            </motion.div>
        </div>
    );
}

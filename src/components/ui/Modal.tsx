/**
 * 弹窗壳：$surface + $stroke 1px r12 padding20 gap12
 * 头：标题 14/600 + 副标 11 $text-3（gap3），可选 40×40 图标盒（Mod Detail），右侧 28×28 返回/关闭
 * 脚：1px $stroke 分隔 + 左摘要文本 11 $text-3 + 右按钮组（gap8）
 * 尺寸是流体的：width/height = 设计稿最小尺寸（窗口再小也不缩），随视口放大到 1.35× 封顶。
 * 四个弹窗共用同一系数，所以「搜索 ↔ 详情」两屏永远同尺寸，跳转不会跳大跳小。
 */
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog";
import { ArrowLeft, X, type LucideIcon } from "lucide-react";
import { type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Divider } from "./Panel";

/** 弹窗最大放大倍率（对设计稿尺寸而言） */
const MODAL_SCALE_MAX = 1.35;

export function ModalShell({
    open,
    onClose,
    width,
    height,
    title,
    titleTag,
    sub,
    icon: Icon,
    iconNode,
    back,
    children,
    footerNote,
    footerActions,
    persistent,
}: {
    open: boolean;
    onClose: () => void;
    /** 最小宽度（px）：设计稿宽度 */
    width: number;
    /** 最小高度（px）：省略则由内容决定 */
    height?: number;
    title: string;
    /** 标题右侧的一枚标签（模组详情视图：标签只挂在模组名上，版本行不再重复） */
    titleTag?: ReactNode;
    sub?: string;
    icon?: LucideIcon;
    /** 自定义头部图标（如模组真实头像）；给了它就替代 icon 的图标壳 */
    iconNode?: ReactNode;
    back?: () => void;
    children: ReactNode;
    footerNote?: string;
    footerActions?: ReactNode;
    /** 防误触：点遮罩/按 Esc 不关闭，只能走按钮（目录勾选弹窗用） */
    persistent?: boolean;
}) {
    const sizeStyle = {
        width: `clamp(${width}px, 62vw, ${Math.round(width * MODAL_SCALE_MAX)}px)`,
        height: height
            ? `clamp(${height}px, 70vh, ${Math.round(height * MODAL_SCALE_MAX)}px)`
            : undefined,
    };

    return (
        <DialogPrimitive.Root
            open={open}
            onOpenChange={(o) => {
                if (!o && !persistent) onClose();
            }}
        >
            <DialogPrimitive.Portal>
                {/* base-ui 在出入场过渡期挂 data-starting/ending-style，配 CSS 过渡做淡入+微缩放 */}
                <DialogPrimitive.Backdrop
                    className={cn(
                        "fixed inset-0 z-50 bg-black/50 transition-opacity duration-200",
                        "data-[starting-style]:opacity-0 data-[ending-style]:opacity-0"
                    )}
                />
                <DialogPrimitive.Popup
                    className={cn(
                        "fixed top-1/2 left-1/2 z-50 flex -translate-x-1/2 -translate-y-1/2 flex-col gap-3 rounded-[12px] border border-stroke bg-surface p-5 outline-none",
                        "transition-[opacity,scale] duration-200",
                        "data-[starting-style]:opacity-0 data-[starting-style]:scale-[0.96]",
                        "data-[ending-style]:opacity-0 data-[ending-style]:scale-[0.96]"
                    )}
                    style={sizeStyle}
                >
                    <div className="flex w-full items-center justify-between gap-2.5">
                        {iconNode ??
                            (Icon && (
                                <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-surface-2">
                                    <Icon className="size-5 text-accent" />
                                </span>
                            ))}
                        <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                            {/* 标题 + 标签一行：标题继续 truncate，标签 shrink-0 不被挤掉 */}
                            <span className="flex min-w-0 items-center gap-2">
                                <DialogPrimitive.Title className="truncate text-[14px] leading-[20px] font-semibold text-text-1">
                                    {title}
                                </DialogPrimitive.Title>
                                {titleTag && <span className="flex shrink-0 items-center">{titleTag}</span>}
                            </span>
                            {sub && (
                                <span className="truncate text-[11px] leading-[16px] font-normal text-text-3">
                                    {sub}
                                </span>
                            )}
                        </div>
                        <div className="flex shrink-0 items-center gap-2">
                            {back && (
                                <button
                                    onClick={back}
                                    title="返回"
                                    className="flex size-7 items-center justify-center rounded-lg border border-stroke bg-surface text-text-2 transition-colors hover:bg-surface-2"
                                >
                                    <ArrowLeft className="size-3.5" />
                                </button>
                            )}
                            <button
                                onClick={onClose}
                                title="关闭"
                                className="flex size-7 items-center justify-center rounded-lg border border-stroke text-text-2 transition-colors hover:bg-surface-2"
                            >
                                <X className="size-3.5" />
                            </button>
                        </div>
                    </div>

                    {children}

                    {(footerNote || footerActions) && (
                        <div className="flex w-full flex-col gap-3">
                            <Divider hard />
                            <div className="flex w-full items-center justify-between gap-2.5">
                                <span className="text-[11px] leading-[16px] font-normal text-text-3">
                                    {footerNote}
                                </span>
                                <div className="flex items-center gap-2">{footerActions}</div>
                            </div>
                        </div>
                    )}
                </DialogPrimitive.Popup>
            </DialogPrimitive.Portal>
        </DialogPrimitive.Root>
    );
}

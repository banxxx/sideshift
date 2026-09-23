/**
 * 行排版原语：卡内一行「标签 + 控件」的几种固定版式
 */
import { type ReactNode } from "react";
import { type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Tip, TIP_TRIGGER } from "./Tip";
import { HOVER_FILL } from "./HoverFill";

/** 卡内单行：左 12/500 $text-1 标签 + 右控件（Convert 运行环境/启动参数各行） */
export function InlineRow({
    label,
    className,
    children,
}: {
    label: string;
    className?: string;
    children: ReactNode;
}) {
    return (
        <div className={cn("flex w-full items-center justify-between gap-4", className)}>
            <span className="min-w-0 text-[12px] leading-[18px] font-medium text-text-1">
                {label}
            </span>
            {children}
        </div>
    );
}

/** 设置分组标题：等宽 11/600 $text-3 + 1.2 字距（转换选项 / 网络 / 外观与关于） */
export function SectionTitle({ children }: { children: ReactNode }) {
    return (
        <span className="font-mono text-[11px] leading-[16px] font-semibold tracking-[1.2px] text-text-3">
            {children}
        </span>
    );
}

/** 设置行 padding[14,20]：左 13/600 $text-1 + 11 $text-3 说明（gap 2），右控件 */
export function SettingRow({
    label,
    desc,
    descMono,
    children,
}: {
    label: string;
    desc?: ReactNode;
    /** 说明行用等宽字体（版本行 "v0.1.0 · build 3 · Tauri 2"） */
    descMono?: boolean;
    children: ReactNode;
}) {
    return (
        <div className="flex w-full items-center justify-between gap-4 px-5 py-[14px]">
            <div className="flex min-w-0 flex-col gap-0.5">
                <span className="text-[13px] leading-[20px] font-semibold text-text-1">
                    {label}
                </span>
                {desc && (
                    <span
                        className={cn(
                            "text-[11px] leading-[16px] font-normal text-text-3",
                            descMono && "font-mono"
                        )}
                    >
                        {desc}
                    </span>
                )}
            </div>
            <div className="flex shrink-0 items-center gap-2">{children}</div>
        </div>
    );
}

/** 提示行：gap 6 + 12px 图标 + 等宽 11 $text-3（Convert 摘要卡 i-row） */
export function NoteRow({
    icon: Icon,
    children,
}: {
    icon: LucideIcon;
    children: ReactNode;
}) {
    return (
        <div className="flex items-center gap-1.5">
            <Icon className="size-3 shrink-0 text-text-3" />
            <span className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                {children}
            </span>
        </div>
    );
}

/**
 * 弹窗列表行容器：padding[8,4] gap10 两端对齐（选中/悬停底色由 className 决定）
 * title 同 Btn：不落 DOM，交给 Tip 画气泡（整宽行往左长会顶出弹窗，故贴左）
 */
export function ListRow({ className, title, children, ...rest }: React.ComponentProps<"div">) {
    return (
        <div
            className={cn(
                "flex w-full items-center gap-2.5 rounded-lg px-1 py-2",
                HOVER_FILL,
                title && TIP_TRIGGER,
                className
            )}
            {...rest}
        >
            {children}
            <Tip label={title} align="start" />
        </div>
    );
}

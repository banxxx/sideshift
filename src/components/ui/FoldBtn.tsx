/**
 * 展开/收起一块内容的方形符号钮：图形就是下拉选择框那枚 `ChevronDown`，
 * 开合状态由 `aria-expanded` 外显、符号转 180°，按钮上不再有「展开」两个字。
 *
 * 为什么单独一个件：关于页有三处用它（鸣谢名单的高度档、数据与隐私、第三方组件），
 * 而「转 180°」这条 transform 必须落在**图标**上、不能落在按钮上——旋转与按钮自己的
 * `active:scale` 同用一条 `transform`，写在一起会互相顶掉。
 *
 * 无可见文字，所以名字靠 `aria-label`（全站已禁原生 title 灰泡）。
 */
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { HOVER_PRESS } from "./HoverFill";

export function FoldBtn({
    open,
    label,
    onClick,
    className,
}: {
    /** 当前是否展开 */
    open: boolean;
    /** 读屏用的名字，调用方给「展开××」/「收起××」这一句（会随语言切） */
    label: string;
    onClick: () => void;
    className?: string;
}) {
    return (
        <button
            type="button"
            aria-expanded={open}
            aria-label={label}
            onClick={onClick}
            className={cn(
                "inline-flex size-7 shrink-0 items-center justify-center rounded-md",
                "text-text-2 hover:bg-surface-2 hover:text-text-1",
                HOVER_PRESS,
                className
            )}
        >
            <ChevronDown
                className={cn(
                    "size-3.5 transition-transform duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)]",
                    open && "rotate-180"
                )}
            />
        </button>
    );
}

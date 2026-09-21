/* ================= 网络添加弹窗的复用件（分段器 / 头像 / 骨架屏） ================= */
import { Puzzle } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { motion } from "motion/react";
import { SEG_PILL_SPRING } from "@/components/ui";
import { cn } from "@/lib/utils";

export type Source = "modrinth" | "curseforge";

/** 下载次数 → "1,204 万" / "8,412" */
export function formatCount(n: number): string {
    return n >= 10_000 ? `${(n / 10_000).toFixed(0)} 万` : n.toLocaleString();
}

/** 下载源分段：176×30 轨道 p2 $surface-2 r8；内项 86×26 r6（选中 $accent + 11/600 $accent-ink） */
export function SourceSeg({ value, onChange }: { value: Source; onChange: (s: Source) => void }) {
    const pillId = useId();
    const items: Array<{ key: Source; label: string }> = [
        { key: "modrinth", label: "Modrinth" },
        { key: "curseforge", label: "CurseForge" },
    ];
    return (
        <div className="flex h-[30px] w-[176px] shrink-0 gap-0.5 rounded-lg bg-surface-2 p-0.5">
            {items.map((it) => {
                const active = it.key === value;
                return (
                    <button
                        key={it.key}
                        onClick={() => onChange(it.key)}
                        className={cn(
                            // 宽度用 flex-1 均分：固定 86px 会超出轨道净宽（176-4-2）挤压圆角
                            "relative flex h-[26px] min-w-0 flex-1 items-center justify-center rounded-md text-[11px] leading-[16px] transition-colors",
                            active
                                ? "font-semibold text-accent-ink"
                                : "font-medium text-text-3 hover:text-text-2"
                        )}
                    >
                        {active && (
                            <motion.span
                                layoutId={`${pillId}-src-pill`}
                                transition={SEG_PILL_SPRING}
                                className="absolute inset-0 rounded-md bg-accent"
                            />
                        )}
                        <span className="relative z-[1]">{it.label}</span>
                    </button>
                );
            })}
        </div>
    );
}

/** 模组头像：真实 iconUrl 直链；无图/加载失败回退拼图占位 */
export function ModIcon({
    url,
    className,
    puzzleClass = "size-4",
}: {
    url?: string;
    className?: string;
    puzzleClass?: string;
}) {
    const [failed, setFailed] = useState(false);
    useEffect(() => setFailed(false), [url]);
    const box = cn(
        "flex shrink-0 items-center justify-center overflow-hidden rounded-lg bg-surface-2",
        className ?? "size-9"
    );
    if (!url || failed) {
        return (
            <span className={box}>
                <Puzzle className={cn(puzzleClass, "text-text-2")} />
            </span>
        );
    }
    return (
        <img
            src={url}
            alt=""
            loading="lazy"
            onError={() => setFailed(true)}
            className={cn(box, "object-cover")}
        />
    );
}

/** 列表骨架屏：请求未回来时占住行高，避免布局跳动 */
export function ListSkeleton({ rows = 6, icon = false }: { rows?: number; icon?: boolean }) {
    return (
        <div className="flex min-h-0 flex-1 flex-col gap-0.5">
            {Array.from({ length: rows }, (_, i) => (
                <div key={i} className="flex h-[46px] shrink-0 animate-pulse items-center gap-3 rounded-lg px-2">
                    {icon && <span className="size-9 shrink-0 rounded-lg bg-surface-2" />}
                    <span className="flex min-w-0 flex-1 flex-col gap-1.5">
                        <span className="h-3 w-2/5 rounded bg-surface-2" />
                        <span className="h-2.5 w-3/5 rounded bg-surface-2" />
                    </span>
                </div>
            ))}
        </div>
    );
}

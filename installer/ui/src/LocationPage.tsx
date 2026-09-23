/**
 * 第 1 页：安装位置
 *
 * 一段 = 一个目录：标题 + 一句它装什么 + 路径框 + 更改。程序安装位置在上（它就是几个文件的
 * 落点，看一眼就走），数据目录在下（会涨到几个 GB，值得慢慢挑）。
 *
 * 候选盘做成「推荐位置」一排短按钮，不做成大列表：列表会把同一串路径显示两遍（选中行的路径
 * 和下面框里那个是一件事），而这里要的只是"换个盘"这个动作。
 *
 * 布局规则只有 Rust 那一份（resolve_layout）：这里不做任何路径拼接，否则
 * "装完发现路径不对"只能靠人肉复现。
 */
import { FolderOpen } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Btn } from "@/components/ui/Button";
import { TextInput } from "@/components/ui/Field";
import { formatSize } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { Layout, Plan } from "./api";

export function LocationPage({
    plan,
    sel,
    installDir,
    error,
    onPickRoot,
    onPickInstallDir,
}: {
    plan: Plan | null;
    sel: Layout | null;
    installDir: string;
    error: string | null;
    onPickRoot: (root: string) => void;
    onPickInstallDir: (dir: string) => void;
}) {
    const drives = plan?.drives ?? [];

    const browse = async (title: string, from: string, pick: (p: string) => void) => {
        const chosen = await open({ directory: true, defaultPath: from, title });
        if (typeof chosen === "string") pick(chosen);
    };

    return (
        <div className="page-in flex flex-col">
            <h1 className="text-[19px] leading-[26px] font-semibold text-text-1">
                安装位置
            </h1>

            {/* 段落在这一层里才是首尾：h1 当兄弟会让 first:/last: 全部落空 */}
            <div className="flex flex-col">
                <Sect title="程序安装位置" why="SideShift.exe 与随附文件">
                    <PathRow
                        label="程序安装位置"
                        value={installDir}
                        onBrowse={() =>
                            void browse(
                                "选择程序安装位置",
                                installDir,
                                onPickInstallDir
                            )
                        }
                    />
                </Sect>

                <Sect title="数据目录" why="整合包缓存与转换产物都在这里，按 GB 计">
                    <PathRow
                        label="数据目录"
                        value={sel?.dataRoot ?? ""}
                        onBrowse={() =>
                            void browse(
                                "选择数据目录",
                                sel?.dataRoot ?? "",
                                onPickRoot
                            )
                        }
                    />
                    {drives.length > 0 && (
                        <div className="mt-[9px] flex flex-wrap items-center gap-1.5">
                            <span className="mr-0.5 text-[11px] leading-[16px] text-text-3">
                                推荐位置
                            </span>
                            {drives.map((d) => (
                                <RootChip
                                    key={d.dataRoot}
                                    label={d.label}
                                    free={formatSize(d.freeBytes)}
                                    active={sel?.dataRoot === d.dataRoot}
                                    onClick={() => onPickRoot(d.dataRoot)}
                                />
                            ))}
                        </div>
                    )}
                    {error && (
                        <p className="mt-2 text-[12px] leading-[18px] text-redstone">
                            {error}
                        </p>
                    )}
                </Sect>
            </div>
        </div>
    );
}

/** 一段：标题 + 一句它装什么 + 控件。段间 1px 线，末段不画（下面就是页脚了） */
function Sect({
    title,
    why,
    children,
}: {
    title: string;
    why: string;
    children: React.ReactNode;
}) {
    return (
        <div className="border-b border-stroke-soft py-4 first:pt-3 last:border-b-0">
            <div className="flex items-baseline gap-2">
                <h2 className="text-[13px] leading-[20px] font-semibold text-text-1">
                    {title}
                </h2>
                <span className="text-[11px] leading-[18px] text-text-3">{why}</span>
            </div>
            {children}
        </div>
    );
}

function PathRow({
    label,
    value,
    onBrowse,
}: {
    label: string;
    value: string;
    onBrowse: () => void;
}) {
    return (
        <div className="mt-[9px] flex items-center gap-2">
            <TextInput
                className="min-w-0 flex-1"
                readOnly
                plain
                icon={FolderOpen}
                value={value}
                aria-label={label}
            />
            <Btn size="sm" icon={FolderOpen} onClick={onBrowse}>
                更改…
            </Btn>
        </div>
    );
}

/** 推荐位置一枚：短到只报"哪个盘、还剩多少"，选中态由 accent 描边 + 底色承担 */
function RootChip({
    label,
    free,
    active,
    onClick,
}: {
    label: string;
    free: string;
    active: boolean;
    onClick: () => void;
}) {
    return (
        <button
            aria-pressed={active}
            onClick={onClick}
            className={cn(
                "flex h-6 shrink-0 items-center gap-[5px] rounded-md border px-2.5 text-[11px] leading-[16px]",
                "transition-colors",
                active
                    ? "border-accent bg-accent-dim font-semibold text-accent"
                    : "border-stroke bg-surface text-text-2 hover:border-text-3 hover:text-text-1"
            )}
        >
            {label} 盘
            <span className={active ? "text-accent/70" : "text-text-3"}>剩 {free}</span>
        </button>
    );
}

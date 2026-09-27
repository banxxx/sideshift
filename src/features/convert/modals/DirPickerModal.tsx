/* ================= 目录勾选弹窗（客户端保留目录卡「添加目录」） ================= */
import { ChevronRight, Folder, MinusSquare, SquareCheck } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { PackDirNode } from "@/lib/types";
import { Btn, CheckBox, ListRow, ModalShell, SearchBox, HOVER_FILL } from "@/components/ui";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** 树内目录节点总数（标题「共 N 个目录」口径） */
function countDirNodes(nodes: PackDirNode[]): number {
    return nodes.reduce((s, n) => s + 1 + countDirNodes(n.children), 0);
}

/** 父子去重：祖先已勾选的目录是冗余项（整个父目录都会保留） */
function pruneRedundant(paths: string[]): string[] {
    const sorted = [...paths].sort();
    return sorted.filter((p) => !sorted.some((q) => q !== p && p.startsWith(`${q}/`)));
}

/** 两条路径是否互为父子/祖孙（`kubejs` 与 `kubejs/client_scripts`）；树上名字已按后端口径小写 */
function isRelative(a: string, b: string): boolean {
    return a !== b && (a.startsWith(`${b}/`) || b.startsWith(`${a}/`));
}

/**
 * 新勾选的一条覆盖与它有父子关系的旧条目：清单里可能同时出现父级与子级（先勾了根级目录、
 * 再进子层勾它下面的某一条，或从旧草稿带进来），此时必须让**刚点的这条**说了算，
 * 否则「应用」时子项会被 `pruneRedundant` 判成冗余项当场丢掉，用户挑的子目录整个没生效。
 * 反方向同理（勾父级就把已勾的子目录收进父级）。
 * 与 `pruneRedundant` 的分工：这里管**用户刚点的那一下**（意图明确，新点的赢）；
 * 那条只管本次没碰过的遗留父子对（方向取"不缩小保留范围"，保父）。
 */
function withPrecedence(v: string[], key: string): string[] {
    // `x !== key`：全选那条路是逐个 reduce，同层里本来已勾的那条会再进来一次，不去重就出现重复条目
    return [...v.filter((x) => x !== key && !isRelative(x, key)), key];
}

/**
 * 层级目录浏览器：双击进入子目录（含子目录的行），单击勾选（延迟判定避让双击）；
 * 头部返回键 + 面包屑回退层级。父子互斥在勾选当场解决（见 `withPrecedence`），
 * 「应用」再兜一次遗留的父子对（见 `pruneRedundant`）后整体回写。
 */
export function DirPickerModal({
    open,
    onClose,
    dirs,
    selected,
    onApply,
}: {
    open: boolean;
    onClose: () => void;
    dirs: PackDirNode[];
    selected: string[];
    onApply: (next: string[]) => void;
}) {
    const t = useT();
    const [query, setQuery] = useState("");
    /** 当前浏览层级（从包根起算的目录段，[]=根） */
    const [path, setPath] = useState<string[]>([]);
    const [draft, setDraft] = useState<string[]>(selected);
    /** 单击延迟句柄：等待第二次点击判定是否双击，避免双击=勾选两次 */
    const clickTimer = useRef<number | null>(null);

    // 每次打开都从当前已选重建暂存与浏览位置（卡片行内移除后再开不会带旧草稿）
    useEffect(() => {
        if (open) {
            setDraft(selected.slice());
            setPath([]);
            setQuery("");
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [open]);

    const levelNodes = useMemo(() => {
        let nodes = dirs;
        for (const seg of path) nodes = nodes.find((n) => n.name === seg)?.children ?? [];
        return nodes;
    }, [dirs, path]);

    const filtered = useMemo(
        () =>
            levelNodes.filter(
                (n) => !query || n.name.toLowerCase().includes(query.toLowerCase())
            ),
        [levelNodes, query]
    );

    /** 行/勾选的唯一键 = 包根起算的相对路径 */
    const keyOf = (n: PackDirNode) => [...path, n.name].join("/");

    const toggle = (key: string) =>
        setDraft((v) => (v.includes(key) ? v.filter((x) => x !== key) : withPrecedence(v, key)));

    const clearTimer = () => {
        if (clickTimer.current !== null) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
        }
    };

    const handleRowClick = (n: PackDirNode) => {
        // 第二次 click 交给 dblclick 处理，这里只撤销挂起的单击
        if (clickTimer.current !== null) {
            clearTimer();
            return;
        }
        const key = keyOf(n);
        if (n.children.length === 0) {
            toggle(key);
            return;
        }
        clickTimer.current = window.setTimeout(() => {
            clickTimer.current = null;
            toggle(key);
        }, 240);
    };

    const handleRowDblClick = (n: PackDirNode) => {
        clearTimer();
        if (n.children.length > 0) setPath((p) => [...p, n.name]);
    };

    // 全选/全取消（切换式）作用于当前层「可见」（含搜索过滤）的目录
    const levelKeys = filtered.map(keyOf);
    const allOn = levelKeys.length > 0 && levelKeys.every((k) => draft.includes(k));
    const toggleAll = () =>
        setDraft((v) =>
            allOn
                ? v.filter((k) => !levelKeys.includes(k))
                : levelKeys.reduce(withPrecedence, v)
        );

    return (
        <ModalShell
            open={open}
            onClose={onClose}
            persistent
            back={path.length > 0 ? () => setPath((p) => p.slice(0, -1)) : undefined}
            width={560}
            height={440}
            title={t("convert-modals.choose-folders", "选择保留目录 · {{count}} 个目录", { count: countDirNodes(dirs) })}
            sub={t("convert-modals.double-click-opens", "双击进入子目录 · 单击勾选 · 勾选后随包复制到服务端")}
            footerNote={t("convert-modals.count-folder", "{{count}} 个目录将随包保留", { count: draft.length })}
            footerActions={
                <>
                    <Btn size="sm" className="px-3.5" onClick={onClose}>
                        {t("common.cancel", "取消")}
                    </Btn>
                    <Btn
                        variant="primary"
                        size="sm"
                        className="px-3.5 font-semibold"
                        onClick={() => {
                            clearTimer();
                            onApply(pruneRedundant(draft));
                            onClose();
                        }}
                    >
                        {t("convert-modals.apply", "应用")}
                    </Btn>
                </>
            }
        >
            {/* 面包屑：全部 › kubejs › …（末段为当前位置，其余可点回退） */}
            <div className="flex w-full shrink-0 flex-wrap items-center gap-1 font-mono text-[11px] leading-[16px] text-text-3">
                <button
                    className={cn(
                        HOVER_FILL,
                        path.length === 0
                            ? "font-medium text-text-1"
                            : "hover:text-text-1"
                    )}
                    onClick={() => setPath([])}
                >
                    {t("common.entry-2", "全部")}
                </button>
                {path.map((seg, i) => (
                    <span key={`${seg}-${i}`} className="flex items-center gap-1">
                        <ChevronRight className="size-3" />
                        <button
                            className={cn(
                                HOVER_FILL,
                                i === path.length - 1
                                    ? "font-medium text-text-1"
                                    : "hover:text-text-1"
                            )}
                            onClick={() => setPath((p) => p.slice(0, i + 1))}
                        >
                            {seg}
                        </button>
                    </span>
                ))}
            </div>

            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder={t("convert-modals.search-folder", "搜索当前层目录名称…")}
                className="border border-stroke"
            />

            {/* toolbar：左全选（切换式，含清空语义） + 右计数 */}
            <div className="flex w-full shrink-0 items-center justify-between gap-2">
                <button
                    className={`flex items-center gap-2 text-[11px] leading-[16px] font-medium text-text-2 hover:text-text-1 ${HOVER_FILL}`}
                    onClick={toggleAll}
                >
                    {allOn ? (
                        <MinusSquare className="size-3.5 text-accent" />
                    ) : (
                        <SquareCheck className="size-3.5 text-accent" />
                    )}
                    {allOn ? t("convert-modals.unselect", "取消全选") : t("convert-modals.select", "全选")}
                </button>
                <span className="font-mono text-[11px] leading-[16px] font-normal tabular-nums text-text-3">
                    {t("convert-modals.checked-total", "已勾选 {{checked}} / {{total}}", {
                        checked: draft.length,
                        total: countDirNodes(dirs),
                    })}
                </span>
            </div>

            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {filtered.map((n) => {
                    const key = keyOf(n);
                    const on = draft.includes(key);
                    return (
                        <ListRow
                            key={key}
                            className="cursor-pointer hover:bg-surface-2"
                            title={n.children.length > 0 ? t("convert-modals.double-click", "双击进入子目录") : undefined}
                            onClick={() => handleRowClick(n)}
                            onDoubleClick={() => handleRowDblClick(n)}
                        >
                            {/* 勾选框即时勾选，不参与双击判定 */}
                            <span onClick={(e) => e.stopPropagation()}>
                                <CheckBox checked={on} onChange={() => toggle(key)} />
                            </span>
                            <Folder
                                className={cn(
                                    "size-4 shrink-0",
                                    on ? "text-accent" : "text-text-3"
                                )}
                            />
                            <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                {n.name}/
                            </span>
                            <span
                                className={cn(
                                    "shrink-0 font-mono text-[11px] leading-[16px] tabular-nums",
                                    on ? "text-emerald" : "text-text-3"
                                )}
                            >
                                {t("convert.count-file", "{{count}} 文件", { count: n.fileCount })}
                            </span>
                        </ListRow>
                    );
                })}
                {filtered.length === 0 && (
                    <span className="py-8 text-center text-[11px] text-text-3">
                        {dirs.length === 0 ? t("convert-modals.folders-keep", "包内没有可保留的目录") : t("convert-modals.matching-folders", "无匹配目录")}
                    </span>
                )}
            </div>
        </ModalShell>
    );
}

/* ============ 保留内容勾选弹窗（客户端保留内容卡「添加内容」） ============ */
import { ChevronRight, FileText, Folder, MinusSquare, SquareCheck } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { PackDirNode, PackDirTree, PackFileNode } from "@/lib/types";
import { baseName, yieldedIn } from "@/lib/types";
import { formatSize } from "@/lib/format";
import { Btn, CheckBox, ListRow, ModalShell, SearchBox, HOVER_FILL } from "@/components/ui";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** 树内目录节点总数（标题口径） */
function countDirNodes(nodes: PackDirNode[]): number {
    return nodes.reduce((s, n) => s + 1 + countDirNodes(n.children), 0);
}

/** 全树可勾的文件数（每个节点的直属文件都可勾，深层的要钻进去才看得见） */
function countFileNodes(nodes: PackDirNode[]): number {
    return nodes.reduce((s, n) => s + n.files.length + countFileNodes(n.children), 0);
}

/** 父子去重：父目录落位时子目录已经在它内部一起进包了，另勾子级等于同一份内容落两处 */
function pruneRedundant(paths: string[]): string[] {
    const sorted = [...paths].sort();
    return sorted.filter((p) => !sorted.some((q) => q !== p && p.startsWith(`${q}/`)));
}

/** 两条勾选键落位后是否撞在产物里的同一个位置：落位名（最后一段）相同就是撞 */
const sameLanding = (a: string, b: string) =>
    a !== b && baseName(a).toLowerCase() === baseName(b).toLowerCase();

/** 去掉与本次勾选落位同名的其它条目（目录档与文件档一起查）；新点的那条说了算 */
function dropLandingClash(paths: string[], key: string): string[] {
    return paths.filter((p) => !sameLanding(p, key));
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
 * 这一行能不能钻进去：有子目录、或有直属文件都算。
 * 只看 `children` 的那版判据把「只装文件的目录」（`shaderpacks/` 这类）整个挡在门外——
 * 弹窗里那条目录行因此永远给不出它凭什么被勾选的依据，而这正是这张卡要展示的简略信息。
 */
const canEnter = (n: PackDirNode) => n.children.length > 0 || n.files.length > 0;

/**
 * 层级浏览器：双击进入下一层（有子目录或有直属文件的行），单击勾选（延迟判定避让双击）；
 * 头部返回键 + 面包屑回退层级。父子互斥在勾选当场解决（见 `withPrecedence`），
 * 「应用」再兜一次遗留的父子对（见 `pruneRedundant`）后整体回写。
 *
 * 目录与文件在**任意层级**都可勾，落位规则是「勾哪一层就剪掉上层」（后端单源 `parser::kept_rel`）：
 * 勾 `config/mods` ⇒ 产物根 `mods/`，勾 `kubejs/startup.js` ⇒ 产物根 `startup.js`，
 * 勾 `config` ⇒ 产物根 `config/` 且内部层级原样保留。于是两条勾选键只要落位名相同就撞到同一个位置
 * ——新点的那条说了算（`dropLandingClash`，目录档与文件档之间同样查）。
 */
export function DirPickerModal({
    open,
    onClose,
    tree,
    selected,
    selectedFiles,
    onApply,
}: {
    open: boolean;
    onClose: () => void;
    tree: PackDirTree;
    selected: string[];
    /** 已勾的根级散文件（逻辑相对路径，与 `tree.files[].path` 同源） */
    selectedFiles: string[];
    onApply: (dirs: string[], files: string[]) => void;
}) {
    const t = useT();
    const [query, setQuery] = useState("");
    /** 当前浏览层级（从包根起算的目录段，[]=根） */
    const [path, setPath] = useState<string[]>([]);
    const [draft, setDraft] = useState<string[]>(selected);
    const [draftFiles, setDraftFiles] = useState<string[]>(selectedFiles);
    /** 单击延迟句柄：等待第二次点击判定是否双击，避免双击=勾选两次 */
    const clickTimer = useRef<number | null>(null);

    // 每次打开都从当前已选重建暂存与浏览位置（卡片行内移除后再开不会带旧草稿）
    useEffect(() => {
        if (open) {
            setDraft(selected.slice());
            setDraftFiles(selectedFiles.slice());
            setPath([]);
            setQuery("");
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [open]);

    /** 当前层的目录与直属文件；根级的文件来自 `tree.files`，深层的来自所在节点 */
    const level = useMemo(() => {
        let dirs = tree.dirs;
        let files = tree.files;
        for (const seg of path) {
            const n = dirs.find((d) => d.name === seg);
            dirs = n?.children ?? [];
            files = n?.files ?? [];
        }
        return { dirs, files };
    }, [tree, path]);

    const match = (name: string) => !query || name.toLowerCase().includes(query.toLowerCase());
    const filtered = level.dirs.filter((n) => match(n.name));
    const filteredFiles = level.files.filter((f) => match(f.name));

    /** 行/勾选的唯一键 = 包根起算的相对路径；落位时剪掉它前面的所有层 */
    const keyOf = (n: PackDirNode) => [...path, n.name].join("/");

    /** 目录档：勾上时收掉父子对（`withPrecedence`）与落位同名的文件条目 */
    const toggle = (key: string) => {
        setDraft((v) => (v.includes(key) ? v.filter((x) => x !== key) : withPrecedence(v, key)));
        // 判据只读最新草稿，不读渲染期的 `draft`：单击是 240ms 后才落地的，那时闭包里的草稿可能已经旧了一轮
        setDraftFiles((v) => (v.some((x) => sameLanding(x, key)) ? dropLandingClash(v, key) : v));
    };

    /** 文件档：同样查落位名——`x/eula.txt` 与包根 `eula.txt` 是同一个位置，勾后者要把前者收掉。
     *  `dropLandingClash` 只会减不会加，所以添加那一条必须先 `[...v, key]` 再拿去撞名过滤 */
    const toggleFile = (key: string) => {
        setDraftFiles((v) =>
            v.includes(key) ? v.filter((x) => x !== key) : dropLandingClash([...v, key], key)
        );
        setDraft((v) => (v.some((x) => sameLanding(x, key)) ? dropLandingClash(v, key) : v));
    };

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
        if (!canEnter(n)) {
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
        if (canEnter(n)) setPath((p) => [...p, n.name]);
    };

    // 全选/全取消（切换式）作用于当前层「可见」（含搜索过滤）的条目，目录与文件两档都算
    const levelDirKeys = filtered.map(keyOf);
    const levelFileKeys = filteredFiles.map((f) => f.path);
    const levelKeys = [...levelDirKeys, ...levelFileKeys];
    const allOn =
        levelKeys.length > 0 &&
        levelKeys.every((k) => draft.includes(k) || draftFiles.includes(k));
    const toggleAll = () => {
        if (allOn) {
            setDraft((v) => v.filter((k) => !levelKeys.includes(k)));
            setDraftFiles((v) => v.filter((k) => !levelKeys.includes(k)));
            return;
        }
        // 两条腿各归各的草稿：文件路径混进 keepDirs 会被当成目录前缀匹配，整个包都留下来了。
        // 每加一条都顺带查落位同名（`x/jei` 与刚勾的 `jei` 在产物里是同一个位置）
        setDraft((v) => levelDirKeys.reduce((acc, k) => dropLandingClash(withPrecedence(acc, k), k), v));
        setDraftFiles((v) => levelFileKeys.reduce((acc, k) => dropLandingClash([...new Set([...acc, k])], k), v));
    };

    const totalCount = countDirNodes(tree.dirs) + countFileNodes(tree.dirs) + tree.files.length;
    const checkedCount = draft.length + draftFiles.length;
    /** 勾中的「让位」包根文件（eula.txt / server.properties）：文件名是 vanilla 写死的，勾了就以包内那份为准，
     *  标题下那一行因此改口——不然用户带着勾回到页面，看到的还是照常可编辑的服务端设置 */
    const yieldedChecked = yieldedIn(draftFiles);

    return (
        <ModalShell
            open={open}
            onClose={onClose}
            persistent
            back={path.length > 0 ? () => setPath((p) => p.slice(0, -1)) : undefined}
            width={560}
            height={440}
            title={t("convert-modals.choose-folders", "选择保留内容 · {{count}} 个目录", { count: countDirNodes(tree.dirs) })}
            sub={
                yieldedChecked.length > 0
                    ? t(
                          "convert-modals.yielded-root",
                          "已勾的 {{names}} 以包内那份为准 · 转换配置里对应的设置项这次不会写进包",
                          { names: yieldedChecked.join("、") }
                      )
                    : t(
                          "convert-modals.double-click-opens",
                          "双击进入子目录 · 单击勾选 · 勾哪一层就按自己的名字落在产物根目录"
                      )
            }
            footerNote={t("convert-modals.count-folder", "已勾 {{dirs}} 个目录 · {{files}} 个文件，将随包保留", {
                dirs: draft.length,
                files: draftFiles.length,
            })}
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
                            onApply(pruneRedundant(draft), [...draftFiles].sort());
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
                placeholder={t("convert-modals.search-folder", "搜索当前层名称…")}
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
                        checked: checkedCount,
                        total: totalCount,
                    })}
                </span>
            </div>

            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-2 overflow-auto px-1">
                {filtered.map((n) => {
                    const key = keyOf(n);
                    const on = draft.includes(key);
                    return (
                        <ListRow
                            key={key}
                            className="rounded-md cursor-pointer hover:bg-surface-2 py-2.5"
                            title={
                                !canEnter(n)
                                    ? undefined
                                    : n.children.length > 0
                                      ? t("convert-modals.double-click", "双击进入子目录")
                                      : t("convert-modals.double-click-files", "双击查看文件")
                            }
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
                                {" · "}
                                {formatSize(n.sizeBytes)}
                            </span>
                        </ListRow>
                    );
                })}
                {/* 文件排在同层目录之后；与目录档一样任意层级可勾，勾上就按自己的名字落产物根 */}
                {filteredFiles.map((f) => (
                    <FileRow
                        key={f.path}
                        file={f}
                        checked={draftFiles.includes(f.path)}
                        onToggle={() => toggleFile(f.path)}
                    />
                ))}
                {filtered.length === 0 && filteredFiles.length === 0 && (
                    <span className="py-8 text-center text-[11px] leading-[16px] text-text-3">
                        {tree.dirs.length === 0 && tree.files.length === 0
                            ? t("convert-modals.folders-keep", "包内没有可保留的目录与文件")
                            : t("convert-modals.matching-folders", "无匹配条目")}
                    </span>
                )}
            </div>
        </ModalShell>
    );
}

/** 文件行：与目录行同一条版（勾选壳 + 图标 + 名 + 右侧大小），只少了「能不能钻进去」 */
function FileRow({
    file,
    checked,
    onToggle,
}: {
    file: PackFileNode;
    checked: boolean;
    onToggle: () => void;
}) {
    const t = useT();
    const on = checked;
    return (
        <ListRow className="rounded-md cursor-pointer hover:bg-surface-2 py-2.5" onClick={onToggle}>
            <span onClick={(e) => e.stopPropagation()}>
                <CheckBox checked={on} onChange={onToggle} />
            </span>
            <FileText className={cn("size-4 shrink-0", on ? "text-accent" : "text-text-3")} />
            <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                {file.name}
            </span>
            <span
                className={cn(
                    "shrink-0 font-mono text-[11px] leading-[16px] tabular-nums",
                    on ? "text-emerald" : "text-text-3"
                )}
            >
                {/* 0 = 后端没测到大小（index 与 zip 条目都缺 fileSize）；宁缺勿假，别写成 0 B */}
                {file.sizeBytes
                    ? formatSize(file.sizeBytes)
                    : t("convert.size-unknown", "大小未知")}
            </span>
        </ListRow>
    );
}

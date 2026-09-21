/* ================= 网络添加弹窗（800×464，St8m8 + 详情 wphzw） ================= */
import { ChevronLeft, ChevronRight, Puzzle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { formatSize, loaderLabel } from "@/lib/format";
import type {
    LoaderKind,
    ModSearchResult,
    ModVersionEntry,
    VersionOption,
} from "@/lib/types";
import { Btn, ListRow, ModalShell, SearchBox, ToneChip } from "@/components/ui";
import { cn } from "@/lib/utils";
import { SideChip } from "./SideChip";
import {
    FilterSelect,
    formatCount,
    ListSkeleton,
    ModIcon,
    SourceSeg,
    type FilterOpt,
    type Source,
} from "./OnlineAddParts";

export function OnlineAddModal({
    open,
    onClose,
    mcVersion,
    loader,
    onAdd,
}: {
    open: boolean;
    onClose: () => void;
    mcVersion: string;
    loader: LoaderKind;
    /** 选中某个构建版本后回写新增列表 */
    onAdd: (mod: ModSearchResult, version: ModVersionEntry) => void;
}) {
    const [view, setView] = useState<"list" | "detail">("list");
    const [source, setSource] = useState<Source>("modrinth");
    const [query, setQuery] = useState("");
    const [debounced, setDebounced] = useState("");
    const [page, setPage] = useState(1);
    // 三筛选值："" = 全部版本；"" = 任意加载器；"all" = 全部类别
    const [verSel, setVerSel] = useState(mcVersion);
    const [loSel, setLoSel] = useState<LoaderKind | "">(loader);
    const [catSel, setCatSel] = useState("all");
    const [mcOptions, setMcOptions] = useState<VersionOption[]>([]);
    const [categories, setCategories] = useState<string[]>([]);
    const [detail, setDetail] = useState<ModSearchResult | null>(null);
    const [versions, setVersions] = useState<ModVersionEntry[]>([]);
    const [versionsLoading, setVersionsLoading] = useState(false);
    const [versionsError, setVersionsError] = useState<string | null>(null);
    const [result, setResult] = useState<{ total: number; results: ModSearchResult[] }>({
        total: 0,
        results: [],
    });
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // 弹窗壳常驻挂载：每次打开重置回一级视图与包自身版本/加载器
    useEffect(() => {
        if (!open) return;
        setView("list");
        setDetail(null);
        setQuery("");
        setDebounced("");
        setPage(1);
        setVerSel(mcVersion);
        setLoSel(loader);
        setCatSel("all");
    }, [open, mcVersion, loader]);

    // 搜索词防抖 300ms：真实后端逐键请求会打爆 Modrinth
    useEffect(() => {
        const t = setTimeout(() => setDebounced(query), 300);
        return () => clearTimeout(t);
    }, [query]);

    // 筛选下拉的真实选项：MC 版本表 + Modrinth 官方类别标签
    useEffect(() => {
        if (!open) return;
        void api.listMcVersions().then(setMcOptions).catch(() => {});
        void api.listModCategories().then(setCategories).catch(() => {});
    }, [open]);

    useEffect(() => {
        if (!open) return;
        let alive = true;
        setLoading(true);
        setError(null);
        void api
            .searchMods({
                source,
                text: debounced,
                mcVersion: verSel,
                loader: loSel || null,
                category: catSel,
                page,
            })
            .then((p) => {
                if (!alive) return;
                setResult({ total: p.total, results: p.results });
                setLoading(false);
            })
            .catch((e: unknown) => {
                if (!alive) return;
                setResult({ total: 0, results: [] });
                setError(e instanceof Error ? e.message : String(e));
                setLoading(false);
            });
        return () => {
            alive = false;
        };
    }, [open, source, debounced, page, verSel, loSel, catSel]);

    const openDetail = (mod: ModSearchResult) => {
        setDetail(mod);
        setView("detail");
        setVersions([]);
        setVersionsError(null);
        setVersionsLoading(true);
        void api
            .listModVersions(mod.id)
            .then(setVersions)
            .catch((e: unknown) => setVersionsError(e instanceof Error ? e.message : String(e)))
            .finally(() => setVersionsLoading(false));
    };

    const close = () => {
        onClose();
        setView("list");
        setDetail(null);
    };

    const verOpts = useMemo<FilterOpt[]>(
        () => [
            { value: "", chip: "全部", label: "全部版本" },
            ...mcOptions.map((o) => ({
                value: o.value,
                chip: o.value,
                label: o.value,
                group: o.group,
            })),
        ],
        [mcOptions]
    );
    const loOpts = useMemo<FilterOpt[]>(
        () => [
            { value: "", chip: "任意", label: "任意加载器" },
            { value: "fabric", chip: "Fabric", label: "Fabric" },
            { value: "forge", chip: "Forge", label: "Forge" },
            { value: "neoforge", chip: "NeoForge", label: "NeoForge" },
        ],
        []
    );
    const catOpts = useMemo<FilterOpt[]>(
        () => [
            { value: "all", chip: "全部", label: "全部类别" },
            ...categories.map((c) => ({ value: c, chip: c, label: c })),
        ],
        [categories]
    );

    // 二级视图共用同一筛选状态：版本行按所选版本/加载器在前端过滤
    const shownVersions = useMemo(
        () =>
            versions.filter(
                (v) => (!verSel || v.mcVersion === verSel) && (!loSel || v.loader === loSel)
            ),
        [versions, verSel, loSel]
    );

    const filterNote = `${verSel ? `Minecraft ${verSel}` : "全部版本"} · ${loSel ? loaderLabel(loSel) : "任意加载器"}`;

    /* 模组名后那一枚端标签（全弹窗只此一处，列表行与版本行都不再挂）：
       优先项目级支持度，平台只在构建级给数据时退到首个有声明的构建；两边都没有就不挂 */
    const modTag = useMemo(() => {
        if (!detail) return null;
        if (detail.clientSide || detail.serverSide) return detail;
        return shownVersions.find((v) => v.clientSide || v.serverSide) ?? null;
    }, [detail, shownVersions]);

    /* ---- 二级视图：模组详情 + 版本列表（整行点击下载） ---- */
    if (view === "detail" && detail) {
        return (
            <ModalShell
                open={open}
                onClose={close}
                persistent
                back={() => setView("list")}
                width={800}
                height={464}
                icon={Puzzle}
                iconNode={<ModIcon url={detail.iconUrl} className="size-10" puzzleClass="size-5" />}
                title={detail.name}
                titleTag={modTag ? <SideChip sides={modTag} warnClient /> : undefined}
                sub={`${source === "modrinth" ? "Modrinth" : "CurseForge"} · 作者 ${detail.author} · ${formatCount(detail.downloads)} 次下载`}
            >
                <p className="shrink-0 text-[13px] leading-[20px] font-normal text-text-2">
                    {detail.description}
                </p>
                <div className="flex h-[30px] w-full shrink-0 items-center gap-2">
                    <div className="flex h-7 items-center gap-2">
                        <FilterSelect
                            prefix="版本"
                            value={verSel}
                            options={verOpts}
                            searchable
                            onChange={setVerSel}
                        />
                        <FilterSelect
                            prefix="加载器"
                            value={loSel}
                            options={loOpts}
                            onChange={(v) => setLoSel(v as LoaderKind | "")}
                        />
                    </div>
                </div>
                <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                    {versionsLoading ? (
                        <ListSkeleton rows={5} />
                    ) : versionsError ? (
                        <span className="py-8 text-center text-[11px] text-gold">
                            版本加载失败 · {versionsError}
                        </span>
                    ) : (
                        <>
                            {shownVersions.map((v, i) => (
                                <ListRow
                                    key={v.id}
                                    className={cn(
                                        "cursor-pointer hover:bg-surface-2",
                                        i === shownVersions.length - 1 && "bg-surface"
                                    )}
                                    onClick={() => {
                                        onAdd(detail, v);
                                        close();
                                    }}
                                >
                                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                        <span className="flex items-center gap-2">
                                            <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                                {v.versionNumber}
                                            </span>
                                            {v.recommended && (
                                                <ToneChip tone="gold" size="xs">
                                                    推荐
                                                </ToneChip>
                                            )}
                                        </span>
                                        <span className="truncate text-[10px] leading-[14px] font-normal text-text-3">
                                            Minecraft {v.mcVersion} · {loaderLabel(v.loader)} ·{" "}
                                            {v.date} · {formatSize(v.sizeBytes)}
                                        </span>
                                    </span>
                                </ListRow>
                            ))}
                            {shownVersions.length === 0 && (
                                <span className="py-8 text-center text-[11px] text-text-3">
                                    {versions.length === 0
                                        ? "该模组没有可用构建"
                                        : "当前筛选下没有构建"}
                                </span>
                            )}
                        </>
                    )}
                </div>
            </ModalShell>
        );
    }

    /* ---- 一级视图：搜索结果列表 ---- */
    return (
        <ModalShell
            open={open}
            onClose={close}
            persistent
            width={800}
            height={464}
            title="从网络添加模组"
            sub={`搜索 Modrinth 与 CurseForge · ${filterNote}`}
            footerNote={
                error
                    ? `${source === "modrinth" ? "Modrinth" : "CurseForge"} · 加载失败`
                    : `${source === "modrinth" ? "Modrinth" : "CurseForge"} · 共 ${result.total} 个结果`
            }
            footerActions={
                <>
                    <Btn
                        size="sm"
                        icon={ChevronLeft}
                        className="w-9 px-0"
                        disabled={page <= 1 || loading}
                        onClick={() => setPage((p) => p - 1)}
                        title="上一页"
                    />
                    <Btn
                        size="sm"
                        icon={ChevronRight}
                        className="w-9 bg-surface px-0"
                        disabled={loading || page * result.results.length >= result.total}
                        onClick={() => setPage((p) => p + 1)}
                        title="下一页"
                    />
                    <Btn variant="primary" size="sm" className="px-3.5 font-semibold" onClick={close}>
                        完成
                    </Btn>
                </>
            }
        >
            <SearchBox
                value={query}
                onChange={(v) => {
                    setQuery(v);
                    setPage(1);
                }}
                placeholder="搜索模组名称…"
                className="bg-surface-2"
            />

            {/* toolbar：下载源分段（176×30）+ 真实筛选下拉组（h28） */}
            <div className="flex h-[30px] w-full shrink-0 items-center justify-between gap-2">
                <SourceSeg
                    value={source}
                    onChange={(s) => {
                        setSource(s);
                        setPage(1);
                    }}
                />
                <div className="flex h-7 items-center gap-2">
                    <FilterSelect
                        prefix="版本"
                        value={verSel}
                        options={verOpts}
                        searchable
                        onChange={(v) => {
                            setVerSel(v);
                            setPage(1);
                        }}
                    />
                    <FilterSelect
                        prefix="加载器"
                        value={loSel}
                        options={loOpts}
                        onChange={(v) => {
                            setLoSel(v as LoaderKind | "");
                            setPage(1);
                        }}
                    />
                    <FilterSelect
                        prefix="类别"
                        value={catSel}
                        options={catOpts}
                        onChange={(v) => {
                            setCatSel(v);
                            setPage(1);
                        }}
                    />
                </div>
            </div>

            {/* 结果行：真实图标 + 整行点入模组详情；请求中显示骨架屏 */}
            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {loading ? (
                    <ListSkeleton rows={6} icon />
                ) : error ? (
                    <span className="py-8 text-center text-[11px] text-gold">
                        加载失败 · {error}
                    </span>
                ) : (
                    <>
                        {result.results.map((m, i) => (
                            <ListRow
                                key={m.id}
                                className={cn(
                                    "cursor-pointer hover:bg-surface-2",
                                    i === result.results.length - 1 && "bg-surface"
                                )}
                                onClick={() => openDetail(m)}
                            >
                                <ModIcon url={m.iconUrl} />
                                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                    <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                        {m.name}
                                    </span>
                                    <span className="truncate text-[10px] leading-[14px] font-normal text-text-3">
                                        {m.description}
                                    </span>
                                </span>
                                {m.alreadyAdded && (
                                    <ToneChip tone="emerald" size="xs">
                                        已添加
                                    </ToneChip>
                                )}
                                <ChevronRight className="size-3.5 shrink-0 text-text-3" />
                            </ListRow>
                        ))}
                        {result.results.length === 0 && (
                            <span className="py-8 text-center text-[11px] text-text-3">
                                无匹配结果
                            </span>
                        )}
                    </>
                )}
            </div>
        </ModalShell>
    );
}

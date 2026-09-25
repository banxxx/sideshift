/* ================= 网络添加弹窗（800×464，St8m8 + 详情 wphzw） ================= */
import { ChevronLeft, ChevronRight, ExternalLink, Puzzle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { formatSize, loaderLabel } from "@/lib/format";
import type {
    LoaderKind,
    ModSearchResult,
    ModVersionEntry,
    VersionOption,
} from "@/lib/types";
import {
    Btn,
    ListRow,
    ModalShell,
    SearchBox,
    SearchSelect,
    type SelectOption,
    ToneChip,
} from "@/components/ui";
import { cn } from "@/lib/utils";
import { SideChip } from "./SideChip";
import {
    formatCount,
    ListSkeleton,
    ModIcon,
    SourceSeg,
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
    /**
     * 初值 true：弹窗壳常驻挂载，首帧时列表还是空的。空 = 「没搜到」还是「没搜过」，
     * 光看 `result` 分不出来，所以由这里显式记着「第一次请求还没落地」，
     * 否则打开瞬间会先闪一帧「无匹配结果 · 共 0 个结果」。
     */
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    /**
     * CurseForge 的 API Key，开弹窗时读一次设置：`null` = 设置还没读到，`""` = 读到了、确实没配。
     * 两者必须分开——把「还没读到」演成「没配」，用户会看到一句假的需要去申请。
     * CurseForge 的每条查询都要这个 Key，缺它 = 平台在门口就拒（403），所以这里宁可不发请求。
     */
    const [cfKey, setCfKey] = useState<string | null>(null);

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
        // 上一次搜索的结果不能在外壳重新挂起的那一帧里露底：先回到「请求中」
        setLoading(true);
        // Key 同理：每次开弹窗重读设置，读回来之前不许按上一次的配置发请求
        setCfKey(null);
    }, [open, mcVersion, loader]);

    // 搜索词防抖 300ms：真实后端逐键请求会打爆 Modrinth
    useEffect(() => {
        const t = setTimeout(() => setDebounced(query), 300);
        return () => clearTimeout(t);
    }, [query]);

    // 筛选下拉的真实选项：MC 版本表 + 当前来源的类别标签（两家词表不同，切来源要重拉）；
    // 顺带读一次设置——CurseForge 那一侧没有用户自己的 Key 就根本发不出可用请求
    useEffect(() => {
        if (!open) return;
        void api.listMcVersions().then(setMcOptions).catch(() => {});
        void api
            .listModCategories(source)
            .then(setCategories)
            .catch(() => setCategories([]));
        void api
            .getSettings()
            .then((s) => setCfKey(s.curseforgeApiKey ?? ""))
            // 读设置都能失败的话，没有更可信的事实可依据了：按「没配」显示出口，
            // 总比卡在「搜索中…」什么都不给看强
            .catch(() => setCfKey(""));
    }, [open, source]);

    useEffect(() => {
        if (!open) return;
        if (source === "curseforge") {
            // 设置还没读回来：整块停在「请求中」，别把「没读到」演成「没配」
            if (cfKey === null) return;
            if (!cfKey.trim()) {
                // 缺 Key 一次请求都不发（发了必被平台拒）：清掉另一家的结果，让结果区只说这一句
                setResult({ total: 0, results: [] });
                setError(null);
                setLoading(false);
                return;
            }
        }
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
    }, [open, source, debounced, page, verSel, loSel, catSel, cfKey]);

    /** 结果区该不该换成「去申请 Key」这句话（与上面那道闸门同一个判据） */
    const cfBlocked = source === "curseforge" && cfKey !== null && !cfKey.trim();
    /** 当前来源的显示名：页脚那句状态与详情页副标题都按它说 */
    const srcName = source === "modrinth" ? "Modrinth" : "CurseForge";

    const openDetail = (mod: ModSearchResult) => {
        setDetail(mod);
        setView("detail");
        setVersions([]);
        setVersionsError(null);
        setVersionsLoading(true);
        void api
            .listModVersions(mod.source, mod.id)
            .then(setVersions)
            .catch((e: unknown) => setVersionsError(e instanceof Error ? e.message : String(e)))
            .finally(() => setVersionsLoading(false));
    };

    const close = () => {
        onClose();
        setView("list");
        setDetail(null);
    };

    const verOpts = useMemo<SelectOption[]>(
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
    const loOpts = useMemo<SelectOption[]>(
        () => [
            { value: "", chip: "任意", label: "任意加载器" },
            { value: "fabric", chip: "Fabric", label: "Fabric" },
            { value: "forge", chip: "Forge", label: "Forge" },
            { value: "neoforge", chip: "NeoForge", label: "NeoForge" },
        ],
        []
    );
    const catOpts = useMemo<SelectOption[]>(
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
                sub={`${srcName} · 作者 ${detail.author} · ${formatCount(detail.downloads)} 次下载`}
            >
                <p className="shrink-0 text-[13px] leading-[20px] font-normal text-text-2">
                    {detail.description}
                </p>
                <div className="flex h-[30px] w-full shrink-0 items-center gap-2">
                    <div className="flex h-7 items-center gap-2">
                        <SearchSelect
                            variant="chip"
                            prefix="版本"
                            value={verSel}
                            options={verOpts}
                            searchable
                            searchPlaceholder="搜索版本…"
                            onChange={setVerSel}
                        />
                        <SearchSelect
                            variant="chip"
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
                cfBlocked
                    ? "CurseForge · 需要 API Key"
                    : error
                      ? `${srcName} · 加载失败`
                      : loading
                        ? `${srcName} · 搜索中…`
                        : `${srcName} · 共 ${result.total} 个结果`
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
                    <SearchSelect
                        variant="chip"
                        prefix="版本"
                        value={verSel}
                        options={verOpts}
                        searchable
                        searchPlaceholder="搜索版本…"
                        onChange={(v) => {
                            setVerSel(v);
                            setPage(1);
                        }}
                    />
                    <SearchSelect
                        variant="chip"
                        prefix="加载器"
                        value={loSel}
                        options={loOpts}
                        onChange={(v) => {
                            setLoSel(v as LoaderKind | "");
                            setPage(1);
                        }}
                    />
                    <SearchSelect
                        variant="chip"
                        prefix="类别"
                        value={catSel}
                        options={catOpts}
                        searchable
                        searchPlaceholder="搜索类别…"
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
                ) : cfBlocked ? (
                    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-1 text-center">
                        <span className="text-[12px] leading-[18px] text-text-2">
                            CurseForge API Key 未配置
                        </span>
                        <span className="text-[11px] leading-[16px] text-text-3">
                            申请后填到「设置 · 网络 · CurseForge API
                            Key」后即可
                        </span>
                        <Btn
                            size="sm"
                            icon={ExternalLink}
                            className="mt-2"
                            onClick={() => void api.openExternal(api.CURSEFORGE_APPLY_FORM)}
                        >
                            去申请 Key
                        </Btn>
                    </div>
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

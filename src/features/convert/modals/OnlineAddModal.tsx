/* ================= 网络添加弹窗（800×464，St8m8 + 详情 wphzw） ================= */
import { ChevronLeft, ChevronRight, Puzzle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { formatSize, loaderLabel, modcatLabel } from "@/lib/format";
import { activeLocale, useT } from "@/lib/i18n";
import { modName } from "@/lib/mod-name";
import { notify } from "@/lib/notify";
import { errOf } from "@/lib/errors";
import type {
    LoaderKind,
    ModDependency,
    ModSearchResult,
    ModTranslation,
    ModVersionEntry,
    VersionOption,
} from "@/lib/types";
import {
    Btn,
    Collapse,
    ListRow,
    ModalShell,
    SearchBox,
    SearchSelect,
    TIP_TRIGGER,
    Tip,
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
    const t = useT();
    /**
     * 详情页栈：从搜索列表进详情是开新链（栈里一页）；点一枚前置是往链上压一页
     * （PCL2 口径，直接进那个前置的详情而不是回搜索）。栈顶就是当前显示的那本；
     * 返回键逐级退，退空了回到列表。压栈的是完整搜索结果，或「先按 id/名字进的半份、
     * 单项目查询到了再原地补全」的前置页
     */
    const [detailStack, setDetailStack] = useState<ModSearchResult[]>([]);
    /** 栈顶即当前详情；派生值——栈空 = 一级列表视图 */
    const detail = detailStack[detailStack.length - 1] ?? null;
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
    const [versions, setVersions] = useState<ModVersionEntry[]>([]);
    const [versionsLoading, setVersionsLoading] = useState(false);
    const [versionsError, setVersionsError] = useState<string | null>(null);
    /**
     * 详情页那栏中文译文（第三方镜像的机翻，点「翻译」才请求）：名与简介各一档覆盖率，
     * 镜像里可能只有其中一个。连着它属于哪一本一起记——回得慢时用户可能已经点进别的模组，
     * 对不上 slug 就当没有，别把上一本的译文挂过来。「拿到了」与「正在显示」分开记
     * ⇒ 切回原文后再点不发第二次请求
     */
    const [zh, setZh] = useState<{ slug: string; tr: ModTranslation } | null>(null);
    const [zhOn, setZhOn] = useState(false);
    const [zhLoading, setZhLoading] = useState(false);
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
    /** 失败原因存后端原文（网络类是 `net:种类:主机` 这种代码），渲染处过 `errOf` 才换语言：
     *  在捕获处就翻译的话，切语言之后这句还挂在屏上、不会跟着变 */
    const [error, setError] = useState<string | null>(null);
    // 弹窗壳常驻挂载：每次打开重置回一级视图与包自身版本/加载器
    useEffect(() => {
        if (!open) return;
        setDetailStack([]);
        setQuery("");
        setDebounced("");
        setPage(1);
        setVerSel(mcVersion);
        setLoSel(loader);
        setCatSel("all");
        // 上一次搜索的结果不能在外壳重新挂起的那一帧里露底：先回到「请求中」
        setLoading(true);
    }, [open, mcVersion, loader]);

    // 搜索词防抖 300ms：真实后端逐键请求会打爆 Modrinth
    useEffect(() => {
        const t = setTimeout(() => setDebounced(query), 300);
        return () => clearTimeout(t);
    }, [query]);

    // 筛选下拉的真实选项：MC 版本表 + 当前来源的类别标签（两家词表不同，切来源要重拉）
    useEffect(() => {
        if (!open) return;
        void api.listMcVersions().then(setMcOptions).catch(() => {});
        void api
            .listModCategories(source)
            .then(setCategories)
            .catch(() => setCategories([]));
    }, [open, source]);

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

    /** 当前来源的显示名：页脚那句状态与详情页副标题都按它说 */
    const srcName = source === "modrinth" ? "Modrinth" : "CurseForge";

    /** 载入某一本的版本列表（进详情与逐级返回共用）：换一本 = 换一句译文，旧译文与显示态都不带过来 */
    const loadVersions = (mod: ModSearchResult) => {
        setVersions([]);
        setVersionsError(null);
        setVersionsLoading(true);
        setZh(null);
        setZhOn(false);
        setZhLoading(false);
        void api
            .listModVersions(mod.source, mod.id)
            .then(setVersions)
            .catch((e: unknown) => setVersionsError(e instanceof Error ? e.message : String(e)))
            .finally(() => setVersionsLoading(false));
    };

    /** 进详情：从列表进入开新链；已在详情里（前置跳转）则把当前这本压栈 */
    const openDetail = (mod: ModSearchResult) => {
        setDetailStack((s) => (detail ? [...s, mod] : [mod]));
        loadVersions(mod);
    };

    /** 返回键：逐级退回上一本详情，退空了回到搜索列表。上一本的版本列表要重新载入 */
    const goBack = () => {
        const next = detailStack.slice(0, -1);
        setDetailStack(next);
        const top = next[next.length - 1];
        if (top) loadVersions(top);
    };

    /**
     * 点一枚前置 = 直跳它的详情页：先用手里已有的字段立即进入（版本列表马上能看），
     * 展示信息（简介/作者/图标/下载量）由一发单项目查询补齐，没补齐前按 id/名字兜底。
     * 回复到达前用户又跳了别的：栈顶 id 对不上就丢弃，别把资料挂错本
     */
    const openDependency = (dep: ModDependency) => {
        if (!detail) return;
        const src = detail.source;
        openDetail({
            id: dep.id,
            slug: dep.slug,
            name: dep.name || dep.slug || dep.id,
            nameZh: dep.nameZh,
            description: "",
            author: "",
            downloads: 0,
            iconUrl: undefined,
            source: src,
            compatible: true,
            alreadyAdded: false,
        });
        void api
            .modDetail(src, dep.id)
            .then((full) => {
                setDetailStack((s) => {
                    const top = s[s.length - 1];
                    if (!top || top.id !== dep.id || top.source !== src) return s;
                    return [...s.slice(0, -1), { ...top, ...full }];
                });
            })
            // 展示信息没补齐也有 id/名字兜底，详情页照常可用（版本列表是独立一路请求）
            .catch(() => {});
    };

    /* 「翻译」这枚钮的门槛：机翻出来的是简体，繁体档给它会露出错体 ⇒ 只在简体中文档出现；
       镜像的 detail 只认 slug（CurseForge 的数字 id 实测查不到），没有 slug 就没有这条线 */
    const canTranslate = activeLocale() === "zh-CN" && !!detail?.slug;
    /** 当前在显示译文的那一份（切回原文时还在，只是不显示，再点不发二次请求） */
    const zhNow = zh && zh.slug === detail?.slug && zhOn ? zh.tr : null;

    const toggleZh = () => {
        const mod = detail;
        const slug = mod?.slug?.trim();
        if (!mod || !slug) return;
        if (zh && zh.slug === slug) {
            setZhOn(!zhOn);
            return;
        }
        setZhLoading(true);
        void api
            .translateModZh(mod.source, slug, mod.name)
            .then((tr) => {
                setZhLoading(false);
                if (!tr) {
                    // 收录有这一本但名与简介都还没译文（镜像那边是空串）：不切态，
                    // 别演成「翻译成功但内容与原文一样」
                    notify(t("convert-modals.no-zh", "这个模组还没有中文译文"));
                    return;
                }
                setZh({ slug, tr });
                setZhOn(true);
            })
            .catch((e: unknown) => {
                setZhLoading(false);
                notify(
                    t("convert-modals.zh-load", "中文译文获取失败 · {{error}}", {
                        error: errOf(e),
                    }),
                    "error"
                );
            });
    };

    const close = () => {
        onClose();
        setDetailStack([]);
    };

    const verOpts = useMemo<SelectOption[]>(
        () => [
            { value: "", chip: t("common.entry-2", "全部"), label: t("convert-modals.versions", "全部版本") },
            ...mcOptions.map((o) => ({
                value: o.value,
                chip: o.value,
                label: o.value,
                // group 参与 SearchSelect 相邻分组判定，必须保持后端原文，不套 t
                group: o.group,
            })),
        ],
        [mcOptions, t]
    );
    const loOpts = useMemo<SelectOption[]>(
        () => [
            { value: "", chip: t("convert-modals.entry-2", "任意"), label: t("convert-modals.loader", "任意加载器") },
            { value: "fabric", chip: "Fabric", label: "Fabric" },
            { value: "forge", chip: "Forge", label: "Forge" },
            { value: "neoforge", chip: "NeoForge", label: "NeoForge" },
        ],
        [t]
    );
    const catOpts = useMemo<SelectOption[]>(
        () => [
            { value: "all", chip: t("common.entry-2", "全部"), label: t("convert-modals.categories", "全部类别") },
            // 类别名来自后端平台词表（Modrinth 为英文 slug，CurseForge 为类目名）：
            // value 恒用原文（走 IPC 的筛选项），显示处 modcatLabel 按当前语言给名字——
            // 词表没登记的长尾原样返回，筛选不受任何影响
            ...categories.map((c) => ({ value: c, chip: modcatLabel(c), label: modcatLabel(c) })),
        ],
        [categories, t]
    );

    // 二级视图共用同一筛选状态：版本行按所选版本/加载器在前端过滤
    const shownVersions = useMemo(
        () =>
            versions.filter(
                (v) => (!verSel || v.mcVersion === verSel) && (!loSel || v.loader === loSel)
            ),
        [versions, verSel, loSel]
    );

    /**
     * 前置行取哪份构建的依赖：推荐构建优先（它被筛掉时退到筛选后的第一个）。
     * 依赖是构建级声明，同一模组各构建的前置极少不同，取参考构建即可代表
     */
    const refVersion = useMemo(
        () => shownVersions.find((v) => v.recommended) ?? shownVersions[0] ?? null,
        [shownVersions]
    );
    const depends = refVersion?.depends ?? [];

    const filterNote = `${verSel ? `Minecraft ${verSel}` : t("convert-modals.versions", "全部版本")} · ${loSel ? loaderLabel(loSel) : t("convert-modals.loader", "任意加载器")}`;

    /* 模组名后那一枚端标签（全弹窗只此一处，列表行与版本行都不再挂）：
       优先项目级支持度，平台只在构建级给数据时退到首个有声明的构建；两边都没有就不挂 */
    const modTag = useMemo(() => {
        if (!detail) return null;
        if (detail.clientSide || detail.serverSide) return detail;
        return shownVersions.find((v) => v.clientSide || v.serverSide) ?? null;
    }, [detail, shownVersions]);

    /* ---- 二级视图：模组详情 + 版本列表（整行点击下载） ---- */
    if (detail) {
        const zhDesc = zhNow?.descriptionZh;
        return (
            <ModalShell
                open={open}
                onClose={close}
                persistent
                back={goBack}
                width={800}
                height={464}
                icon={Puzzle}
                iconNode={<ModIcon url={detail.iconUrl} className="size-10" puzzleClass="size-5" />}
                title={modName(detail, zhNow?.titleZh)}
                titleTag={modTag ? <SideChip sides={modTag} warnClient /> : undefined}
                sub={
                    detail.author
                        ? t("convert-modals.src-author", "{{src}} · 作者 {{author}} · {{downloads}} 次下载", {
                              src: srcName,
                              author: detail.author,
                              downloads: formatCount(detail.downloads),
                          })
                        // 直跳的前置页作者可能没查到（成员表挂了）：省掉那一段，别渲染出「作者 」悬空
                        : t("convert-modals.src-downloads", "{{src}} · {{downloads}} 次下载", {
                              src: srcName,
                              downloads: formatCount(detail.downloads),
                          })
                }
            >
                {/* 简介一栏就地换译文：两条各一个 Collapse 反向开关、同一条曲线，所以高度沿同一条
                    插值走过去，不会「先塌一下再撑开」。外层普通 div 让父级那格 gap 只算一次（两格都在
                    时不会多出 12px）。镜像只翻出名、简介没翻时这一栏原样不动，只有标题跟着换 */}
                <div className="flex shrink-0 flex-col">
                    <Collapse when={!zhDesc}>
                        <p className="text-[13px] leading-[20px] font-normal text-text-2">
                            {detail.description}
                        </p>
                    </Collapse>
                    <Collapse when={!!zhDesc}>
                        <p className="text-[13px] leading-[20px] font-normal text-text-2">
                            {zhDesc}
                        </p>
                    </Collapse>
                </div>
                <div className="flex h-[30px] w-full shrink-0 items-center gap-2">
                    <div className="flex h-7 items-center gap-2">
                        <SearchSelect
                            variant="chip"
                            prefix={t("settings.version", "版本")}
                            value={verSel}
                            options={verOpts}
                            searchable
                            searchPlaceholder={t("convert.search-versions", "搜索版本…")}
                            onChange={setVerSel}
                        />
                        <SearchSelect
                            variant="chip"
                            prefix={t("home.loader", "加载器")}
                            value={loSel}
                            options={loOpts}
                            onChange={(v) => setLoSel(v as LoaderKind | "")}
                        />
                        {canTranslate && (
                            <Btn
                                size="xs"
                                disabled={zhLoading}
                                onClick={toggleZh}
                                /* 挨着两枚 chip 下拉，就照 chip 那一族的样子画（h7 / rounded-md / bg-surface /
                                   11px 主文本色）；只用 Btn 的默认 outline 会比它们轻一档，读起来不像同一排控件 */
                                className="rounded-md bg-surface px-2 text-text-1"
                            >
                                {zhLoading
                                    ? t("convert-modals.zh-loading", "翻译中…")
                                    : zhNow
                                      ? t("convert-modals.zh-original", "原文")
                                      : t("convert-modals.zh-translate", "翻译")}
                            </Btn>
                        )}
                    </div>
                </div>
                {/* 前置模组一行：数据来自版本列表接口本就带回的构建级 dependencies[]，
                    名字由后端一发批量反查补上。放在下载选择正上方——选中某份构建前先看清
                    它要什么。点一枚前置 = 直跳它的详情页（PCL2 口径），在那里还能顺手把它加进方案；
                    返回键逐级退回。悬浮说明用项目自己的 Tip，不挂原生 title */}
                {!versionsLoading && depends.length > 0 && (
                    <div className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] leading-[16px]">
                        <span className="font-medium text-text-3">
                            {t("convert-modals.depends", "前置模组")}
                        </span>
                        {depends.map((d) => (
                            <button
                                key={d.id}
                                type="button"
                                aria-label={t("convert-modals.depends-open", "查看前置详情")}
                                className={cn(
                                    "max-w-[220px] cursor-pointer truncate rounded bg-surface px-1.5 py-0.5 text-text-2 transition-colors hover:bg-surface-2 hover:text-text-1",
                                    TIP_TRIGGER
                                )}
                                onClick={() => openDependency(d)}
                            >
                                <Tip
                                    label={t("convert-modals.depends-open", "查看前置详情")}
                                    align="center"
                                />
                                {modName(d) || d.slug || d.id}
                                {!d.required && (
                                    <span className="ml-1 text-text-3">
                                        {t("convert-modals.depends-optional", "可选")}
                                    </span>
                                )}
                            </button>
                        ))}
                    </div>
                )}
                <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                    {versionsLoading ? (
                        <ListSkeleton rows={5} />
                    ) : versionsError ? (
                        <span className="py-8 text-center text-[11px] text-gold">
                            {t("convert-modals.version-load", "版本加载失败 · {{error}}", { error: errOf(versionsError) })}
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
                                                    {t("common.recommended", "推荐")}
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
                                        ? t("convert-modals.builds-mod", "该模组没有可用构建")
                                        : t("convert-modals.builds-match", "当前筛选下没有构建")}
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
            title={t("convert-modals.add-mods", "从网络添加模组")}
            sub={t("convert-modals.search-modrinth", "搜索 Modrinth 与 CurseForge · {{filter}}", { filter: filterNote })}
            footerNote={
                error
                    ? t("convert-modals.src-load", "{{src}} · 加载失败", { src: srcName })
                    : loading
                      ? t("convert-modals.src-searching", "{{src}} · 搜索中…", { src: srcName })
                      : t("convert-modals.src-count", "{{src}} · 共 {{count}} 个结果", { src: srcName, count: result.total })
            }
            footerActions={
                <>
                    <Btn
                        size="sm"
                        icon={ChevronLeft}
                        className="w-9 px-0"
                        disabled={page <= 1 || loading}
                        onClick={() => setPage((p) => p - 1)}
                        title={t("convert-modals.previous", "上一页")}
                    />
                    <Btn
                        size="sm"
                        icon={ChevronRight}
                        className="w-9 bg-surface px-0"
                        disabled={loading || page * result.results.length >= result.total}
                        onClick={() => setPage((p) => p + 1)}
                        title={t("convert-modals.next", "下一页")}
                    />
                    <Btn variant="primary" size="sm" className="px-3.5 font-semibold" onClick={close}>
                        {t("convert-modals.entry-3", "完成")}
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
                placeholder={t("convert-modals.search-mod", "搜索模组名称…")}
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
                        prefix={t("settings.version", "版本")}
                        value={verSel}
                        options={verOpts}
                        searchable
                        searchPlaceholder={t("convert.search-versions", "搜索版本…")}
                        onChange={(v) => {
                            setVerSel(v);
                            setPage(1);
                        }}
                    />
                    <SearchSelect
                        variant="chip"
                        prefix={t("home.loader", "加载器")}
                        value={loSel}
                        options={loOpts}
                        onChange={(v) => {
                            setLoSel(v as LoaderKind | "");
                            setPage(1);
                        }}
                    />
                    <SearchSelect
                        variant="chip"
                        prefix={t("convert-modals.category", "类别")}
                        value={catSel}
                        options={catOpts}
                        searchable
                        searchPlaceholder={t("convert-modals.search-category", "搜索类别…")}
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
                        {t("convert-modals.load-failed", "加载失败 · {{error}}", { error: errOf(error) })}
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
                                        {modName(m)}
                                    </span>
                                    <span className="truncate text-[10px] leading-[14px] font-normal text-text-3">
                                        {m.description}
                                    </span>
                                </span>
                                {m.alreadyAdded && (
                                    <ToneChip tone="emerald" size="xs">
                                        {t("convert-modals.added", "已添加")}
                                    </ToneChip>
                                )}
                                <ChevronRight className="size-3.5 shrink-0 text-text-3" />
                            </ListRow>
                        ))}
                        {result.results.length === 0 && (
                            <span className="py-8 text-center text-[11px] text-text-3">
                                {t("convert-modals.results", "无匹配结果")}
                            </span>
                        )}
                    </>
                )}
            </div>
        </ModalShell>
    );
}

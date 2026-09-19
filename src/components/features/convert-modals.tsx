/**
 * Convert 页弹窗族（SS.pen `PCRJi` 剔除清单 / `St8m8` 网络添加 / `wphzw` 模组详情）
 *
 * 定稿规则：
 *  - 遮罩 50% 黑；模态 $surface + $stroke 1px r12 padding20 gap12；页脚 = 1px $stroke + 摘要 + 按钮组
 *  - 网络添加与模组详情是同壳二级视图，尺寸强制一致 800×464（“跳转后弹窗不能变小”）
 *  - 处置清单壳剔除/保留共用（focus 区分）：行勾选语义 = 是否处于该处置；
 *    取消勾选即「反向待办」（金色行 + 描边徽章）
 */
import { ChevronDown, ChevronLeft, ChevronRight, Puzzle, RefreshCw, Square, SquareCheck } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { formatSize, loaderLabel } from "@/lib/format";
import type {
    LoaderKind,
    ModDisposition,
    ModSearchResult,
    ModVersionEntry,
    PlanMod,
} from "@/lib/types";
import {
    Btn,
    ListRow,
    ModalShell,
    SEG_PILL_SPRING,
    SearchBox,
    TagChip,
    ToneChip,
} from "@/components/design/ui";
import { cn } from "@/lib/utils";

/* ================= 处置清单弹窗（640 宽，PCRJi 剔除态；保留态共用同壳） ================= */

export type ListFocus = Extract<ModDisposition, "remove" | "keep">;

/** 同壳双清单的文案与语义表 */
const LIST_COPY: Record<
    ListFocus,
    {
        title: string;
        sub: string;
        selectAll: string;
        toolbar: (on: number, off: number) => string;
        note: (on: number, off: number) => string;
        more: (rest: number) => string;
        offBadge: string;
        rowOff: string;
    }
> = {
    remove: {
        title: "剔除清单",
        sub: "检测器识别的客户端专属模组 · 可逐项恢复保留",
        selectAll: "全选剔除",
        toolbar: (on, off) => `已标记 ${on} · 保留 ${off}`,
        note: (on, off) => `${on} 项将剔除 · ${off} 项改为保留`,
        more: (rest) => `… 其余 ${rest} 项均为客户端专属模组`,
        offBadge: "待恢复",
        rowOff: "已手动改为保留 · 服务端将不剔除",
    },
    keep: {
        title: "保留清单",
        sub: "将随服务端包构建的模组 · 可逐项改回剔除",
        selectAll: "全选保留",
        toolbar: (on, off) => `保留 ${on} · 已改剔除 ${off}`,
        note: (on, off) => `${on} 项将保留 · ${off} 项改为剔除`,
        more: (rest) => `… 其余 ${rest} 项均为服务端可用模组`,
        offBadge: "待剔除",
        rowOff: "已手动改为剔除 · 不再进入服务端包",
    },
};

/** 处于该清单处置下的行说明（剔除态沿用设计稿口径） */
function rowOnSub(m: PlanMod, focus: ListFocus): string {
    if (focus === "remove") {
        return m.needsReview
            ? "剔除原因：客户端/服务端两可用，默认按客户端处理"
            : "剔除原因：客户端专属（env=client）";
    }
    return m.autoSupplement
        ? "自动补齐的服务端依赖 · 剔除可能导致启动失败"
        : "服务端可用 · 随包构建";
}

export function PlanListModal({
    open,
    onClose,
    focus,
    mods,
    onDisposition,
}: {
    open: boolean;
    onClose: () => void;
    /** 清单视角：remove=剔除态为“开”，keep=保留态为“开” */
    focus: ListFocus;
    /** 该清单的全部候选，含当前处置 */
    mods: PlanMod[];
    /** 勾选态变更：在 focus 与其反方向之间切换 */
    onDisposition: (id: string, d: ModDisposition) => void;
}) {
    const copy = LIST_COPY[focus];
    const other: ModDisposition = focus === "remove" ? "keep" : "remove";
    const [query, setQuery] = useState("");
    const filtered = useMemo(
        () => mods.filter((m) => !query || m.name.toLowerCase().includes(query.toLowerCase())),
        [mods, query]
    );

    const on = mods.filter((m) => m.disposition === focus).length;
    const off = mods.length - on;

    return (
        <ModalShell
            open={open}
            onClose={onClose}
            width={640}
            height={480}
            title={`${copy.title} · ${mods.length} 个模组`}
            sub={copy.sub}
            footerNote={copy.note(on, off)}
            footerActions={
                <>
                    <Btn size="sm" className="px-3.5" onClick={() => filtered.forEach((m) => onDisposition(m.id, focus))}>
                        {copy.selectAll}
                    </Btn>
                    <Btn variant="primary" size="sm" className="px-3.5 font-semibold" onClick={onClose}>
                        应用
                    </Btn>
                </>
            }
        >
            <SearchBox
                value={query}
                onChange={setQuery}
                placeholder="搜索模组名称…"
                className="border border-stroke"
            />

            {/* toolbar：左全选 + 右计数 */}
            <div className="flex w-full shrink-0 items-center justify-between gap-2">
                <button
                    className="flex items-center gap-2 text-[11px] leading-[16px] font-medium text-text-2 hover:text-text-1"
                    onClick={() => filtered.forEach((m) => onDisposition(m.id, focus))}
                >
                    <SquareCheck className="size-3.5 text-accent" />
                    全选
                </button>
                <span className="font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {copy.toolbar(on, off)}
                </span>
            </div>

            {/* list：gap2；行 padding[8,4] */}
            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {filtered.map((m) => {
                    const isOn = m.disposition === focus;
                    return (
                        <ListRow
                            key={m.id}
                            className={cn("cursor-pointer", !isOn && "bg-gold-dim")}
                            onClick={() => onDisposition(m.id, isOn ? other : focus)}
                        >
                            {isOn ? (
                                <SquareCheck className="size-[15px] shrink-0 text-accent" />
                            ) : (
                                <Square className="size-[15px] shrink-0 text-gold" />
                            )}
                            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                                <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                    {m.name} {m.version}
                                </span>
                                <span
                                    className={cn(
                                        "truncate text-[10px] leading-[14px] font-normal",
                                        isOn ? "text-text-3" : "text-gold"
                                    )}
                                >
                                    {isOn ? rowOnSub(m, focus) : copy.rowOff}
                                </span>
                            </span>
                            {isOn ? (
                                focus === "remove" ? (
                                    <TagChip square>客户端专属</TagChip>
                                ) : m.autoSupplement ? (
                                    <TagChip square>自动补齐</TagChip>
                                ) : (
                                    <TagChip square>服务端保留</TagChip>
                                )
                            ) : (
                                <TagChip square outline className="text-gold">
                                    {copy.offBadge}
                                </TagChip>
                            )}
                        </ListRow>
                    );
                })}
                {filtered.length === 0 && (
                    <span className="py-8 text-center text-[11px] text-text-3">无匹配模组</span>
                )}
                {filtered.length > 4 && (
                    <div className="flex w-full justify-center py-1.5">
                        <span className="text-[11px] leading-[16px] font-normal text-text-3">
                            {copy.more(filtered.length - 4)}
                        </span>
                    </div>
                )}
            </div>
        </ModalShell>
    );
}

/* ================= 网络添加弹窗（800×464，St8m8 + 详情 wphzw） ================= */

type Source = "modrinth" | "curseforge";

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
    const [page, setPage] = useState(1);
    const [detail, setDetail] = useState<ModSearchResult | null>(null);
    const [versions, setVersions] = useState<ModVersionEntry[]>([]);
    const [result, setResult] = useState<{ total: number; results: ModSearchResult[] }>({
        total: 0,
        results: [],
    });

    useEffect(() => {
        if (!open) return;
        let alive = true;
        void api.searchMods({ source, text: query, mcVersion, loader, page }).then((p) => {
            if (alive) setResult({ total: p.total, results: p.results });
        });
        return () => {
            alive = false;
        };
    }, [open, source, query, page, mcVersion, loader]);

    const openDetail = (mod: ModSearchResult) => {
        setDetail(mod);
        setView("detail");
        void api.listModVersions(mod.id).then(setVersions);
    };

    const close = () => {
        onClose();
        setView("list");
        setDetail(null);
    };

    const loaderName = loaderLabel(loader);

    /* ---- 二级视图：模组详情 + 版本列表（整行点击下载） ---- */
    if (view === "detail" && detail) {
        return (
            <ModalShell
                open={open}
                onClose={close}
                back={() => setView("list")}
                width={800}
                height={464}
                icon={Puzzle}
                title={detail.name}
                sub={`${source === "modrinth" ? "Modrinth" : "CurseForge"} · 作者 ${detail.author} · ${formatCount(detail.downloads)} 次下载`}
            >
                <p className="shrink-0 text-[13px] leading-[20px] font-normal text-text-2">
                    {detail.description}
                </p>
                <div className="flex h-[30px] w-full shrink-0 items-center gap-2">
                    <div className="flex h-7 items-center gap-2">
                        <FilterChip label={`版本 ${mcVersion}`} />
                        <FilterChip label={`加载器 ${loaderName}`} />
                    </div>
                </div>
                <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                    {versions.map((v, i) => (
                        <ListRow
                            key={v.id}
                            className={cn(
                                "cursor-pointer hover:bg-surface-2",
                                i === versions.length - 1 && "bg-surface"
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
                                    Minecraft {v.mcVersion} · {loaderLabel(v.loader)} · {v.date} ·{" "}
                                    {formatSize(v.sizeBytes)}
                                </span>
                            </span>
                        </ListRow>
                    ))}
                    {versions.length === 0 && (
                        <span className="flex items-center justify-center gap-1.5 py-8 text-center text-[11px] text-text-3">
                            <RefreshCw className="size-3 animate-spin" />
                            加载版本中…
                        </span>
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
            width={800}
            height={464}
            title="从网络添加模组"
            sub={`搜索 Modrinth 与 CurseForge · 自动匹配 Minecraft ${mcVersion} · ${loaderName}`}
            footerNote={`${source === "modrinth" ? "Modrinth" : "CurseForge"} · 共 ${result.total} 个结果`}
            footerActions={
                <>
                    <Btn
                        size="sm"
                        icon={ChevronLeft}
                        className="w-9 px-0"
                        disabled={page <= 1}
                        onClick={() => setPage((p) => p - 1)}
                        title="上一页"
                    />
                    <Btn
                        size="sm"
                        icon={ChevronRight}
                        className="w-9 bg-surface px-0"
                        disabled={page * result.results.length >= result.total}
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

            {/* toolbar：下载源分段（176×30）+ 筛选 chip 组（h28） */}
            <div className="flex h-[30px] w-full shrink-0 items-center justify-between gap-2">
                <SourceSeg
                    value={source}
                    onChange={(s) => {
                        setSource(s);
                        setPage(1);
                    }}
                />
                <div className="flex h-7 items-center gap-2">
                    <FilterChip label={`版本 ${mcVersion}`} />
                    <FilterChip label={`加载器 ${loaderName}`} />
                    <FilterChip label="类别 全部" />
                </div>
            </div>

            {/* 结果行：整行点入模组详情 */}
            <div className="-mx-1 flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto px-1">
                {result.results.map((m, i) => (
                    <ListRow
                        key={m.id}
                        className={cn(
                            "cursor-pointer hover:bg-surface-2",
                            i === result.results.length - 1 && "bg-surface"
                        )}
                        onClick={() => openDetail(m)}
                    >
                        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-surface-2">
                            <Puzzle className="size-4 text-text-2" />
                        </span>
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
                    <span className="py-8 text-center text-[11px] text-text-3">无匹配结果</span>
                )}
            </div>
        </ModalShell>
    );
}

/** 下载源分段：176×30 轨道 p2 $surface-2 r8；内项 86×26 r6（选中 $accent + 11/600 $accent-ink） */
function SourceSeg({ value, onChange }: { value: Source; onChange: (s: Source) => void }) {
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
                            "relative flex h-[26px] w-[86px] items-center justify-center rounded-md text-[11px] leading-[16px] transition-colors",
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

/** 筛选 chip：h28 gap4 padding[0,10] $surface + $stroke 1px r6，11/500 $text-1 + chevron-down 12 */
function FilterChip({ label }: { label: string }) {
    return (
        <button className="inline-flex h-7 shrink-0 items-center gap-1 rounded-md border border-stroke bg-surface px-2.5 text-[11px] leading-[16px] font-medium text-text-1 transition-colors hover:bg-surface-2">
            {label}
            <ChevronDown className="size-3 text-text-2" />
        </button>
    );
}

/** 下载次数 → "1,204 万" / "8,412" */
function formatCount(n: number): string {
    return n >= 10_000 ? `${(n / 10_000).toFixed(0)} 万` : n.toLocaleString();
}

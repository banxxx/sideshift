/**
 * 转换配置页 Convert（SS.pen `dEsbp` 剔除态 / `pRF47` 新增态 / `sc3I9` 下拉展开态）
 *
 * checkout 式骨架：BodyRow gap20 = 左列（gap16，三张卡各 gap14）+ 右栏 280px 摘要卡。
 * 全局默认值来自设置页（api.defaultOptions），本页做的是“单包覆写”——离开即丢弃。
 *
 * 模组方案的处置编辑模型：
 *  - plan（后端/mock 给的原始方案）+ extras（本页新增的模组）为数据源
 *  - overrides 记录用户对 remove/keep 的改动，removedAdds 记录被取消勾选的新增项
 *  - 计数与摘要卡一律由这三者派生，保证「Tab 计数 = 摘要计数 = 实际方案」不漂移
 */
import { AlertTriangle, Archive, ChevronRight, Download, File, Folder, Globe, Info, Layers, Plus, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion, type Variants } from "motion/react";
import * as api from "@/lib/api";
import { useNavigation } from "@/lib/navigation";
import { formatSize, loaderLabel, outputNameOf, truncateMiddle } from "@/lib/format";
import type {
    AppSettings,
    ConversionOptions,
    ModDisposition,
    ModSearchResult,
    ModVersionEntry,
    PackDirNode,
    PackManifest,
    PlanMod,
    VersionOption,
} from "@/lib/types";
import {
    Btn,
    CheckBox,
    CountRow,
    Divider,
    InlineRow,
    LinkBtn,
    NoteRow,
    PageHeader,
    Panel,
    PanelHead,
    SearchSelect,
    SegTabs,
    Stepper,
    TagChip,
    TextInput,
    Toggle,
    ToneChip,
    type SelectOption,
} from "@/components/design/ui";
import { DirPickerModal, OnlineAddModal, PlanListModal, type ListFocus } from "@/components/features/convert-modals";
import { cn } from "@/lib/utils";

/** 服务端推荐保留目录：包树顶层探测到即预勾选（小写比对） */
const KEEP_DIR_PRESETS = ["config", "defaultconfigs", "kubejs"];

/** VersionOption → 下拉项（group/recommended 透传，供分组与「推荐」标记） */
const toOption = (v: VersionOption): SelectOption => ({
    value: v.value,
    label: v.label,
    recommended: v.recommended,
    group: v.group,
});

/** 按相对路径（kubejs/client_scripts）在目录树中定位节点，取递归文件数；查不到返回 undefined */
function findDirNode(nodes: PackDirNode[], path: string): PackDirNode | undefined {
    const [head, ...rest] = path.split("/");
    const n = nodes.find((x) => x.name.toLowerCase() === head.toLowerCase());
    if (!n || rest.length === 0) return n;
    return n.children.length ? findDirNode(n.children, rest.join("/")) : undefined;
}

/** 卡片内直接展示的行数：卡高 280 下头部+gap+底部出口外，行区（行 20 + 距 14）
 *  在新增态（出口 36px 最高）恰好放得下 5 行；第 6 行需 190px 会被 overflow 裁切。
 *  撤销条/依赖警告出现时行区压缩，超出部分同样裁掉，其余走「查看全部」弹窗 */
const PREVIEW_ROWS = 5;

/** 页面入场：容器管节奏，各卡依次上浮（与 Home idle 分支同一支弹簧手感） */
const PAGE_RISE: Variants = {
    hidden: {},
    show: { transition: { staggerChildren: 0.06, delayChildren: 0.04 } },
};
const CARD_RISE: Variants = {
    hidden: { opacity: 0, y: 14 },
    show: { opacity: 1, y: 0, transition: { type: "spring", stiffness: 320, damping: 28 } },
};

const GAMEMODE_OPTIONS: SelectOption[] = [
    { value: "survival", label: "生存" },
    { value: "creative", label: "创造" },
    { value: "adventure", label: "冒险" },
    { value: "spectator", label: "旁观" },
];

const DIFFICULTY_OPTIONS: SelectOption[] = [
    { value: "peaceful", label: "和平" },
    { value: "easy", label: "简单" },
    { value: "normal", label: "普通" },
    { value: "hard", label: "困难" },
];

export function ConvertPage() {
    const { entry, navigate, switchPrimary } = useNavigation();
    const manifest = entry.params?.manifest as PackManifest | undefined;

    const [options, setOptions] = useState<ConversionOptions | null>(null);
    const [plan, setPlan] = useState<PlanMod[]>([]);
    const [extras, setExtras] = useState<PlanMod[]>([]);
    const [overrides, setOverrides] = useState<Record<string, ModDisposition>>({});
    const [removedAdds, setRemovedAdds] = useState<string[]>([]);
    /** 最近被取消勾选的新增项：给依赖警告 + 撤销出口（自动补齐项文案更重） */
    const [dropped, setDropped] = useState<PlanMod | null>(null);
    const [tab, setTab] = useState<ModDisposition>("remove");
    const [mcOptions, setMcOptions] = useState<SelectOption[]>([]);
    const [loaderOptions, setLoaderOptions] = useState<SelectOption[]>([]);
    const [javaOptions, setJavaOptions] = useState<SelectOption[]>([]);
    // 处置清单弹窗：null=关；remove/keep 决定壳的视角（剔除/保留共用一壳）
    const [listFocus, setListFocus] = useState<ListFocus | null>(null);
    const [onlineOpen, setOnlineOpen] = useState(false);
    const [starting, setStarting] = useState(false);
    /** 全局设置：摘要卡展示默认输出目录（本次覆写为空时回落它） */
    const [settings, setSettings] = useState<AppSettings | null>(null);
    /** 包内可保留目录树（目录勾选弹窗数据源） */
    const [packDirs, setPackDirs] = useState<PackDirNode[]>([]);
    const [dirModalOpen, setDirModalOpen] = useState(false);

    // 进入页面：默认选项（以包的 MC 版本为准）+ 模组方案 + 版本下拉数据
    // keepDirs 预勾选：与目录树一并加载后探测包内存在的推荐目录；用户已有选择则不覆盖
    useEffect(() => {
        if (!manifest) return;
        void Promise.all([api.defaultOptions(manifest), api.listPackDirs()]).then(([o, nodes]) => {
            setPackDirs(nodes);
            const present = KEEP_DIR_PRESETS.filter((name) =>
                nodes.some((n) => n.name.toLowerCase() === name)
            );
            setOptions({
                ...o,
                mcVersion: manifest.mcVersion,
                keepDirs: o.keepDirs.length ? o.keepDirs : present,
            });
        });
        void api.getPlan().then(setPlan);
        void api.listMcVersions().then((l) => setMcOptions(l.map(toOption)));
        void api.listJavaVersions().then((l) => setJavaOptions(l.map(toOption)));
        void api.getSettings().then(setSettings);
    }, [manifest]);

    // MC 版本变更 → 重新拉取该版本可用的加载器版本
    const mcVersion = options?.mcVersion;
    useEffect(() => {
        if (!mcVersion) return;
        void api.listLoaderVersions(mcVersion).then((l) => setLoaderOptions(l.map(toOption)));
    }, [mcVersion]);

    // 裸 zip 无 dependencies 段 → loaderVersion 为空；列表到位后回落推荐项（无推荐取首项），用户可再改
    const loaderVersion = options?.loaderVersion;
    useEffect(() => {
        if (loaderVersion || loaderOptions.length === 0) return;
        const rec = loaderOptions.find((o) => o.recommended) ?? loaderOptions[0];
        setOptions((o) => (o && !o.loaderVersion ? { ...o, loaderVersion: rec.value } : o));
    }, [loaderVersion, loaderOptions]);

    /** 当前生效方案：原始方案 + 本页新增，套用用户改动，剔除被取消的新增项 */
    const mods = useMemo(() => {
        return [...plan, ...extras]
            .filter((m) => !removedAdds.includes(m.id))
            .map((m) => ({ ...m, disposition: overrides[m.id] ?? m.disposition }));
    }, [plan, extras, overrides, removedAdds]);

    const counts = useMemo(
        () => ({
            remove: mods.filter((m) => m.disposition === "remove").length,
            keep: mods.filter((m) => m.disposition === "keep").length,
            add: mods.filter((m) => m.disposition === "add").length,
        }),
        [mods]
    );

    /** 反向依赖警告：保留/新增项依赖了被剔除的行（mrpack depends 元数据，按缺失项聚合） */
    const depWarnings = useMemo(() => {
        const byId = new Map(mods.map((m) => [m.id, m]));
        const groups = new Map<string, { missing: PlanMod; hosts: PlanMod[] }>();
        for (const m of mods) {
            if (m.disposition === "remove") continue;
            for (const d of m.depends ?? []) {
                const t = byId.get(d);
                if (t && t.disposition === "remove") {
                    const g = groups.get(d) ?? { missing: t, hosts: [] };
                    g.hosts.push(m);
                    groups.set(d, g);
                }
            }
        }
        return [...groups.values()];
    }, [mods]);

    /** 本地 .jar 添加的模组 id（徽章显示「本地」而非「推荐」） */
    const localIds = useMemo(
        () => new Set(extras.filter((m) => m.id.startsWith("local-")).map((m) => m.id)),
        [extras]
    );

    const setDisposition = (id: string, d: ModDisposition) =>
        setOverrides((o) => ({ ...o, [id]: d }));

    /** 卡片行内移除单个保留目录（批量增删走 DirPickerModal 应用回写） */
    const removeDir = (name: string) => {
        patch({ keepDirs: (options?.keepDirs ?? []).filter((d) => d !== name) });
    };

    /** 新增项取消勾选 = 从方案移除，并留下可撤销的提示条 */
    const dropAdded = (m: PlanMod) => {
        setRemovedAdds((ids) => [...ids, m.id]);
        setDropped(m);
    };

    const undoDrop = (m: PlanMod) => {
        setRemovedAdds((ids) => ids.filter((id) => id !== m.id));
        setDropped(null);
    };

    const addLocal = async () => {
        const path = await api.pickJarFile();
        if (!path) return;
        const fileName = path.split(/[\\/]/).pop() ?? path;
        setExtras((e) => [
            ...e,
            {
                id: `local-${fileName}`,
                name: fileName.replace(/\.jar$/i, ""),
                version: fileName,
                disposition: "add",
                clientOnly: false,
                needsReview: false,
                autoSupplement: false,
                localPath: path,
            },
        ]);
        setTab("add");
    };

    /** 在线添加：选中某个构建版本后回写新增列表 */
    const addOnline = (mod: ModSearchResult, version: ModVersionEntry) => {
        setExtras((e) =>
            e.some((m) => m.id === mod.id)
                ? e
                : [
                      ...e,
                      {
                          id: mod.id,
                          name: mod.name,
                          version: version.versionNumber,
                          loader: loaderLabel(version.loader),
                          disposition: "add",
                          clientOnly: false,
                          needsReview: false,
                          autoSupplement: false,
                      },
                  ]
        );
        setTab("add");
    };

    const start = async () => {
        if (!manifest || !options || starting) return;
        setStarting(true);
        try {
            const taskId = await api.startConversion(options, manifest, mods);
            navigate("task", { taskId });
        } catch {
            setStarting(false);
        }
    };

    if (!manifest) {
        return (
            <div className="flex flex-col gap-5">
                <PageHeader title="转换配置" />
                <Panel className="items-center py-16">
                    <Layers className="size-6 text-text-3" />
                    <p className="text-[13px] text-text-2">还没有选择整合包，无法配置转换。</p>
                    <Btn variant="primary" size="sm" className="mt-1" onClick={() => switchPrimary("home")}>
                        返回首页选择整合包
                    </Btn>
                </Panel>
            </div>
        );
    }

    const rows = mods.filter((m) => m.disposition === tab).slice(0, PREVIEW_ROWS);
    const loader = loaderLabel(manifest.loader);
    const patch = (p: Partial<ConversionOptions>) => setOptions((o) => (o ? { ...o, ...p } : o));

    /** 本次输出目录 = 单包覆写 ?? 全局设置 */
    const outputOverride = options?.outputOverride?.trim() ?? "";
    const effectiveOutputDir = outputOverride || settings?.outputDir || "";

    const chooseOutputDir = async () => {
        const dir = await api.pickDirectory();
        if (dir) patch({ outputOverride: dir });
    };

    return (
        <motion.div
            className="flex flex-col gap-5 overflow-hidden"
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
        >
            <motion.div variants={CARD_RISE}>
                <PageHeader
                    title="转换配置"
                    sub={`${truncateMiddle(manifest.fileName, 34)} · ${loader} · Minecraft ${manifest.mcVersion} · 检测完成，确认转换方案后开始构建`}
                />
            </motion.div>

            <div className="flex items-start gap-5">
                {/* 左列：运行环境 / 模组方案 / 启动参数（卡间 16） */}
                <div className="flex min-w-0 flex-1 flex-col gap-4">
                    {/* ---- 运行环境：三个版本下拉 + 启动脚本开关 ---- */}
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <Panel gap={14}>
                            <PanelHead title="运行环境" />
                            <div className="flex w-full gap-3">
                                <SearchSelect
                                    className="flex-1"
                                    label="Minecraft 版本"
                                    value={options?.mcVersion ?? manifest.mcVersion}
                                    options={mcOptions}
                                    onChange={(v) => patch({ mcVersion: v })}
                                />
                                <SearchSelect
                                    className="flex-1"
                                    label={`${loader} Loader`}
                                    value={options?.loaderVersion ?? ""}
                                    options={loaderOptions}
                                    onChange={(v) => patch({ loaderVersion: v })}
                                />
                                <SearchSelect
                                    className="flex-1"
                                    label="Java 版本"
                                    value={options?.javaVersion ?? ""}
                                    options={javaOptions}
                                    onChange={(v) => patch({ javaVersion: v })}
                                />
                            </div>
                            <Divider />
                            <InlineRow label="生成启动脚本（start.sh / start.bat）">
                                <Toggle
                                    checked={options?.generateScripts ?? true}
                                    onChange={(v) => patch({ generateScripts: v })}
                                />
                            </InlineRow>
                        </Panel>
                    </motion.div>

                    {/* ---- 模组方案：分段 Tab + 预览行 + 出口 ----
                        卡高固定：切 Tab / 行数变化 / 底部出口块高低不同时，差值全部由
                        行区（flex-1）吸收，卡片外形不再随内容抖动 */}
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <Panel gap={14} className="h-[280px]">
                            <PanelHead
                                title="模组方案"
                                right={
                                    <SegTabs
                                        items={[
                                            { key: "remove" as ModDisposition, label: "剔除", count: counts.remove },
                                            { key: "keep" as ModDisposition, label: "保留", count: counts.keep },
                                            { key: "add" as ModDisposition, label: "新增", count: counts.add },
                                        ]}
                                        value={tab}
                                        onChange={setTab}
                                    />
                                }
                            />

                            {/* 行区：切 Tab 时旧列表上滑退场、新列表下方升入；行从顶部铺开，行数差异由底部空白吸收 */}
                            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
                                <AnimatePresence mode="wait" initial={false}>
                                    <motion.div
                                        key={tab}
                                        initial={{ opacity: 0, y: 10 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -10 }}
                                        transition={{ duration: 0.18, ease: "easeOut" }}
                                        className="flex h-full flex-col justify-start gap-3.5"
                                    >
                                        {rows.length === 0 ? (
                                            <p className="text-center text-[11px] text-text-3">
                                                该分类下暂无模组
                                            </p>
                                        ) : (
                                            rows.map((m) => (
                                                <PlanModRow
                                                    key={m.id}
                                                    mod={m}
                                                    badge={badgeFor(m, localIds.has(m.id))}
                                                    onToggle={() =>
                                                        m.disposition === "add"
                                                            ? dropAdded(m)
                                                            : setDisposition(
                                                                  m.id,
                                                                  m.disposition === "remove"
                                                                      ? "keep"
                                                                      : "remove"
                                                              )
                                                    }
                                                />
                                            ))
                                        )}
                                    </motion.div>
                                </AnimatePresence>
                            </div>

                            {/* 移除撤销条：出现/消失做淡入下滑，切新的移除项时整体重放 */}
                            <AnimatePresence initial={false}>
                                {dropped && (
                                    <motion.div
                                        key={dropped.id}
                                        initial={{ opacity: 0, y: -8 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -8 }}
                                        transition={{ duration: 0.18, ease: "easeOut" }}
                                        className={cn(
                                            "flex shrink-0 items-center gap-2 rounded-lg px-3 py-2",
                                            dropped.autoSupplement ? "bg-gold-dim" : "bg-surface-2"
                                        )}
                                    >
                                        <AlertTriangle
                                            className={cn(
                                                "size-3.5 shrink-0",
                                                dropped.autoSupplement ? "text-gold" : "text-text-3"
                                            )}
                                        />
                                        <span
                                            className={cn(
                                                "min-w-0 flex-1 text-[11px] leading-[16px]",
                                                dropped.autoSupplement ? "text-gold" : "text-text-2"
                                            )}
                                        >
                                            {dropped.autoSupplement
                                                ? `已移除 ${dropped.name}：它是服务端必需前置，缺失可能导致启动失败`
                                                : `已移除 ${dropped.name}：不再加入服务端包`}
                                        </span>
                                        <LinkBtn size="sm" onClick={() => undoDrop(dropped)}>
                                            撤销
                                        </LinkBtn>
                                    </motion.div>
                                )}
                            </AnimatePresence>

                            {/* 反向依赖警告：保留项依赖了被剔除模组，一键恢复缺失项即消警 */}
                            <AnimatePresence initial={false}>
                                {depWarnings.length > 0 && (
                                    <motion.div
                                        key="dep-warn"
                                        initial={{ opacity: 0, y: -8 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -8 }}
                                        transition={{ duration: 0.18, ease: "easeOut" }}
                                        className="flex shrink-0 flex-col gap-1 rounded-lg bg-gold-dim px-3 py-2"
                                    >
                                        {depWarnings.slice(0, 2).map(({ missing, hosts }) => (
                                            <div key={missing.id} className="flex items-center gap-2">
                                                <AlertTriangle className="size-3.5 shrink-0 text-gold" />
                                                <span className="min-w-0 flex-1 truncate text-[11px] leading-[16px] text-gold">
                                                    {hosts
                                                        .slice(0, 2)
                                                        .map((h) => h.name)
                                                        .join("、")}
                                                    {hosts.length > 2 ? ` 等 ${hosts.length} 项` : ""} 依赖被剔除的{" "}
                                                    {missing.name}
                                                </span>
                                                <LinkBtn
                                                    size="sm"
                                                    onClick={() => setDisposition(missing.id, "keep")}
                                                >
                                                    恢复
                                                </LinkBtn>
                                            </div>
                                        ))}
                                        {depWarnings.length > 2 && (
                                            <span className="pl-[22px] text-[10px] leading-[14px] text-gold">
                                                … 另有 {depWarnings.length - 2} 组依赖冲突，可逐项恢复处理
                                            </span>
                                        )}
                                    </motion.div>
                                )}
                            </AnimatePresence>

                            {/* 底部出口区：剔除/保留 → 查看清单链接；新增 → 两枚添加按钮 */}
                            <div className="flex shrink-0 flex-col">
                                <AnimatePresence mode="wait" initial={false}>
                                    <motion.div
                                        key={tab}
                                        initial={{ opacity: 0, y: 8 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -6 }}
                                        transition={{ duration: 0.18, ease: "easeOut" }}
                                        className="flex w-full flex-col"
                                    >
                                        {tab === "remove" && (
                                            <LinkBtn
                                                chevron
                                                className="self-start"
                                                onClick={() => setListFocus("remove")}
                                            >
                                                查看全部 {counts.remove} 项剔除清单
                                            </LinkBtn>
                                        )}
                                        {tab === "keep" && (
                                            <LinkBtn
                                                chevron
                                                className="self-start"
                                                onClick={() => setListFocus("keep")}
                                            >
                                                查看全部 {counts.keep} 项保留清单
                                            </LinkBtn>
                                        )}
                                        {tab === "add" && (
                                            <>
                                                {/* add-btns：整行居中，两枚 h32 padding[0,18] 按钮 */}
                                                <div className="flex h-9 w-full items-center justify-center gap-2.5">
                                                    <Btn
                                                        size="sm"
                                                        icon={File}
                                                        className="px-[18px] font-semibold text-text-1"
                                                        onClick={() => void addLocal()}
                                                    >
                                                        从本地添加
                                                    </Btn>
                                                    <Btn
                                                        variant="primary"
                                                        size="sm"
                                                        icon={Globe}
                                                        className="px-[18px] font-semibold"
                                                        onClick={() => setOnlineOpen(true)}
                                                    >
                                                        从网络添加
                                                    </Btn>
                                                </div>
                                                <p className="mt-3.5 w-full text-center text-[10px] leading-[14px] font-normal text-text-3">
                                                    移除「自动补齐」模组会导致依赖它的客户端模组失效
                                                </p>
                                            </>
                                        )}
                                    </motion.div>
                                </AnimatePresence>
                            </div>
                        </Panel>
                    </motion.div>

                    {/* ---- 客户端保留目录：默认空态，「添加目录」弹窗主动勾选 ---- */}
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <Panel gap={14}>
                            <PanelHead
                                title="客户端保留目录"
                                right={
                                    <Btn
                                        size="sm"
                                        icon={Plus}
                                        disabled={packDirs.length === 0}
                                        onClick={() => setDirModalOpen(true)}
                                    >
                                        添加目录
                                    </Btn>
                                }
                            />
                            {(options?.keepDirs ?? []).length === 0 ? (
                                <p className="w-full py-3 text-center text-[11px] text-text-3">
                                    {packDirs.length === 0
                                        ? "包内未检测到可保留的目录（mods 之外没有资源文件）"
                                        : "尚未选择目录 · 点击上方「添加目录」从包内勾选"}
                                </p>
                            ) : (
                                <div className="flex w-full flex-col gap-1">
                                    <AnimatePresence initial={false} mode="popLayout">
                                        {(options?.keepDirs ?? []).map((p) => {
                                            const dir = findDirNode(packDirs, p);
                                            return (
                                                <motion.div
                                                    key={p}
                                                    layout
                                                    initial={{ opacity: 0, y: -6 }}
                                                    animate={{ opacity: 1, y: 0 }}
                                                    exit={{ opacity: 0, y: -6 }}
                                                    transition={{
                                                        type: "spring",
                                                        stiffness: 320,
                                                        damping: 28,
                                                    }}
                                                    className="flex min-w-0 items-center gap-2.5 rounded-lg px-1.5 py-1 transition-colors hover:bg-surface-2"
                                                >
                                                    <Folder className="size-3.5 shrink-0 text-accent" />
                                                    <span className="min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                                                        {p}/
                                                    </span>
                                                    {dir && (
                                                        <span className="shrink-0 font-mono text-[11px] leading-[16px] tabular-nums text-emerald">
                                                            {dir.fileCount} 文件
                                                        </span>
                                                    )}
                                                    <button
                                                        onClick={() => removeDir(p)}
                                                        title="移除"
                                                        className="flex size-6 shrink-0 items-center justify-center rounded-md text-text-3 transition-colors hover:bg-redstone-dim hover:text-redstone"
                                                    >
                                                        <X className="size-3" />
                                                    </button>
                                                </motion.div>
                                            );
                                        })}
                                    </AnimatePresence>
                                </div>
                            )}
                            <NoteRow icon={Info}>
                                勾选的目录按原层级从源包复制到服务端（支持子目录）；模组配置在 config 目录，不勾选则不带入
                            </NoteRow>
                        </Panel>
                    </motion.div>

                    {/* ---- 启动参数：内存步进器 + 开关 + JVM 参数扩展 ---- */}
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <Panel gap={14}>
                            <PanelHead title="启动参数" />
                            <InlineRow label="服务器内存上限">
                                <Stepper
                                    value={Math.round((options?.memoryMb ?? 6144) / 1024)}
                                    min={1}
                                    max={32}
                                    suffix="GB"
                                    onChange={(v) => patch({ memoryMb: v * 1024 })}
                                />
                            </InlineRow>
                            <InlineRow label="无界面模式启动（--nogui）">
                                <Toggle checked={options?.nogui ?? false} onChange={(v) => patch({ nogui: v })} />
                            </InlineRow>
                            <InlineRow label="自动写入 eula=true（同意 Mojang EULA）">
                                <Toggle
                                    checked={options?.agreeEula ?? true}
                                    onChange={(v) => patch({ agreeEula: v })}
                                />
                            </InlineRow>
                            <InlineRow label="Aikar's flags 优化参数组（G1GC 推荐）">
                                <Toggle
                                    checked={options?.useAikarFlags ?? false}
                                    onChange={(v) => patch({ useAikarFlags: v })}
                                />
                            </InlineRow>
                            <InlineRow label="附加 JVM 参数">
                                <TextInput
                                    className="w-[240px]"
                                    value={options?.extraJvmArgs ?? ""}
                                    onChange={(e) => patch({ extraJvmArgs: e.target.value })}
                                    placeholder="原样拼入 start 脚本"
                                    spellCheck={false}
                                />
                            </InlineRow>
                        </Panel>
                    </motion.div>

                    {/* ---- 服务端设置：server.properties 高频字段（包内自带同名文件时不覆盖） ---- */}
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <Panel gap={14}>
                            <PanelHead title="服务端设置" />
                            <div className="flex w-full gap-3">
                                <SearchSelect
                                    className="flex-1"
                                    label="游戏模式"
                                    value={options?.gamemode ?? "survival"}
                                    options={GAMEMODE_OPTIONS}
                                    onChange={(v) =>
                                        patch({ gamemode: v as ConversionOptions["gamemode"] })
                                    }
                                />
                                <SearchSelect
                                    className="flex-1"
                                    label="难度"
                                    value={options?.difficulty ?? "easy"}
                                    options={DIFFICULTY_OPTIONS}
                                    onChange={(v) =>
                                        patch({ difficulty: v as ConversionOptions["difficulty"] })
                                    }
                                />
                            </div>
                            <div className="grid w-full grid-cols-2 gap-3">
                                <Field label="服务器端口">
                                    <NumField
                                        value={options?.serverPort ?? 25565}
                                        min={1}
                                        max={65535}
                                        onCommit={(v) => patch({ serverPort: v })}
                                    />
                                </Field>
                                <Field label="最大人数">
                                    <NumField
                                        value={options?.maxPlayers ?? 20}
                                        min={1}
                                        max={1000}
                                        onCommit={(v) => patch({ maxPlayers: v })}
                                    />
                                </Field>
                            </div>
                            <Field label="服务器描述（MOTD）">
                                <TextInput
                                    className="w-full"
                                    value={options?.motd ?? ""}
                                    onChange={(e) => patch({ motd: e.target.value })}
                                    placeholder="显示在服务器列表中的一行描述"
                                    spellCheck={false}
                                />
                            </Field>
                            <Field label="世界种子（留空 = 随机生成）">
                                <TextInput
                                    className="w-full"
                                    value={options?.levelSeed ?? ""}
                                    onChange={(e) => patch({ levelSeed: e.target.value })}
                                    placeholder="如 4045151867437057206"
                                    spellCheck={false}
                                />
                            </Field>
                            <Divider />
                            <InlineRow label="正版验证（online-mode）">
                                <Toggle
                                    checked={options?.onlineMode ?? true}
                                    onChange={(v) => patch({ onlineMode: v })}
                                />
                            </InlineRow>
                            <NoteRow icon={Info}>
                                以上字段写入包内 server.properties；整合包自带该文件时保留原文件
                            </NoteRow>
                        </Panel>
                    </motion.div>
                </div>

                {/* 右栏：转换摘要（280px 固定宽） */}
                <motion.aside variants={CARD_RISE} className="flex w-[280px] shrink-0 flex-col gap-4">
                    <Panel gap={14}>
                        <PanelHead title="转换摘要" />
                        <CountRow label="剔除客户端模组" count={counts.remove} tone="gold" />
                        <CountRow label="保留服务端模组" count={counts.keep} tone="emerald" />
                        <CountRow label="新增服务端模组" count={counts.add} tone="accent" />
                        <Divider />
                        <NoteRow icon={Download}>
                            预计下载 {formatSize((counts.keep + counts.add) * 1_200_000)}
                        </NoteRow>
                        <NoteRow icon={Archive}>输出 {outputNameOf(manifest.fileName)}</NoteRow>
                        <NoteRow icon={Folder}>
                            {effectiveOutputDir ? truncateMiddle(effectiveOutputDir, 26) : "默认输出目录"}
                        </NoteRow>
                        <div className="flex w-full items-center justify-between gap-2">
                            <LinkBtn size="sm" onClick={() => void chooseOutputDir()}>
                                {outputOverride ? "更换本次目录…" : "本次改用其他目录…"}
                            </LinkBtn>
                            {!!outputOverride && (
                                <LinkBtn size="sm" onClick={() => patch({ outputOverride: "" })}>
                                    恢复全局
                                </LinkBtn>
                            )}
                        </div>
                        <Btn
                            variant="primary"
                            full
                            disabled={!options || starting || !options.loaderVersion.trim()}
                            onClick={() => void start()}
                        >
                            {starting ? "创建任务中…" : "开始转换"}
                            {!starting && <ChevronRight className="size-[13px]" />}
                        </Btn>
                        <Btn size="sm" full className="font-medium" onClick={() => switchPrimary("home")}>
                            返回首页
                        </Btn>
                        <p className="w-full text-center text-[10px] leading-[14px] font-normal text-text-3">
                            {options && !options.loaderVersion.trim()
                                ? "正在获取 Loader 版本列表，选定后方可开始转换"
                                : "转换过程可随时取消，已下载依赖自动缓存复用"}
                        </p>
                    </Panel>
                </motion.aside>
            </div>

            <PlanListModal
                open={listFocus !== null}
                onClose={() => setListFocus(null)}
                focus={listFocus ?? "remove"}
                // 剔除窗只展示已剔除的模组，保留窗只展示已保留的（弹窗内全部列出，滚动）
                mods={
                    (listFocus ?? "remove") === "remove"
                        ? mods.filter((m) => m.disposition === "remove")
                        : mods.filter((m) => m.disposition === "keep")
                }
                onDisposition={setDisposition}
            />
            <OnlineAddModal
                open={onlineOpen}
                onClose={() => setOnlineOpen(false)}
                mcVersion={options?.mcVersion ?? manifest.mcVersion}
                loader={manifest.loader}
                onAdd={addOnline}
            />
            <DirPickerModal
                open={dirModalOpen}
                onClose={() => setDirModalOpen(false)}
                dirs={packDirs}
                selected={options?.keepDirs ?? []}
                onApply={(next) => patch({ keepDirs: next })}
            />
        </motion.div>
    );
}

/* ---------------- 方案行（mod-row）：勾选框 16 + 名称/版本横排 + 右侧徽章 ---------------- */

function PlanModRow({
    mod,
    badge,
    onToggle,
}: {
    mod: PlanMod;
    badge?: React.ReactNode;
    onToggle: () => void;
}) {
    // 勾选语义 = 进入服务端包；剔除项未勾选，需人工确认项给金色描边
    const included = mod.disposition !== "remove";

    return (
        <div className="flex w-full items-center gap-2.5">
            <CheckBox checked={included} review={!included && mod.needsReview} onChange={onToggle} />
            <div className="flex min-w-0 flex-1 items-center gap-2">
                <span className="truncate font-mono text-[12px] leading-[18px] font-medium text-text-1">
                    {mod.name}
                </span>
                <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {mod.version}
                    {mod.loader ? ` · ${mod.loader}` : ""}
                </span>
            </div>
            {badge}
        </div>
    );
}

/** 徽章优先级：自动补齐 > 需人工确认 > 本地 > 客户端专属 > 推荐（新增项） */
function badgeFor(mod: PlanMod, local: boolean): React.ReactNode {
    if (mod.autoSupplement) return <TagChip>自动补齐</TagChip>;
    if (mod.needsReview) return <ToneChip tone="gold" size="sm">需人工确认</ToneChip>;
    if (local) return <TagChip>本地</TagChip>;
    if (mod.clientOnly) return <TagChip>客户端专属</TagChip>;
    if (mod.disposition === "add") return <TagChip className="text-gold">推荐</TagChip>;
    return undefined;
}

/* ---------------- 服务端设置卡的字段小件 ---------------- */

function Field({ label, children }: { label: string; children: React.ReactNode }) {
    return (
        <label className="flex min-w-0 flex-1 flex-col gap-1.5">
            <span className="text-[11px] leading-[16px] font-medium text-text-2">{label}</span>
            {children}
        </label>
    );
}

/** 数字输入：本地草稿允许瞬时空串/半成品，失焦时 clamp 提交回 options */
function NumField({
    value,
    min,
    max,
    onCommit,
}: {
    value: number;
    min: number;
    max: number;
    onCommit: (v: number) => void;
}) {
    const [draft, setDraft] = useState(String(value));
    useEffect(() => setDraft(String(value)), [value]);
    return (
        <TextInput
            className="w-full"
            inputMode="numeric"
            value={draft}
            onChange={(e) => setDraft(e.target.value.replace(/\D/g, "").slice(0, 6))}
            onBlur={() => {
                const n = parseInt(draft, 10);
                const c = Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : value;
                setDraft(String(c));
                onCommit(c);
            }}
        />
    );
}

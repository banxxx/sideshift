/**
 * 转换配置页 Convert（SS.pen `dEsbp` 剔除态 / `pRF47` 新增态 / `sc3I9` 下拉展开态）
 *
 * checkout 式骨架：BodyRow gap20 = 左列（gap16，三张卡各 gap14）+ 右栏 280px 摘要卡。
 * 全局默认值来自设置页（api.defaultOptions），本页做的是“单包覆写”——离开即丢弃。
 *
 * 模组方案的处置编辑模型：
 *  - plan（后端/mock 给的原始方案）+ extras（本页新增的模组）为数据源
 *  - overrides 记录用户对 remove/keep 的改动；disabledIds 记录被停用的新增行（行保留、不构建）
 *  - 计数/摘要/下发后端的方案一律由 plan+extras+overrides+disabledIds 派生，保证口径不漂移
 */
import { AlertTriangle, Archive, ChevronRight, Download, File, Folder, Globe, Info, Layers, Plus, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion, type Variants } from "motion/react";
import * as api from "@/lib/api";
import { useNavigation } from "@/lib/navigation";
import { notify } from "@/lib/notify";
import { evidenceLabel, formatSize, loaderLabel, outputNameOf, truncateMiddle } from "@/lib/format";
import type {
    AppSettings,
    ConversionOptions,
    DownloadEstimate,
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

/** 卡片内直接展示的行数：卡高 280（内容 240 = p-5 后）− 头 36 − 距 14 = 190 给行区。
 *  行区内部按「行 5×20 + 距 4×10 + 间 10 + 出口（18~32）」≤ 182 排布，出口钉在行区底，
 *  余量只落在列表与出口之间；依赖警告出现时压缩行区（仅行可收缩裁切），
 *  其余走「查看全部」弹窗 */
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
    /** 被停用的新增行 id：取消勾选=停用（行保留在清单，不参与构建），替代旧的「移除+撤销」链路 */
    const [disabledIds, setDisabledIds] = useState<Set<string>>(() => new Set());
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
    /** 自动分类进行中：离线层是同步返回，在线层补全后走 classified 事件再刷一次 */
    const [classifying, setClassifying] = useState(false);
    /** 「清空我的修改」两段式确认（弹窗纪律：不用遮罩/确认框，第二次点击才执行） */
    const [confirmClear, setConfirmClear] = useState(false);

    /** 自动分类主入口：进页默认执行，「重新自动分类」手动再跑一次。
     *  手动处置存在 overrides，方案整体替换也不会覆盖用户改动。
     *  命令返回只代表离线层跑完；联网反查在后台补全，收尾靠 classified 事件 */
    async function runClassify(manual: boolean) {
        setClassifying(true);
        try {
            const res = await api.classifyPack();
            setPlan(res.plan);
            setClassifying(res.onlinePending);
            if (manual) {
                const remove = res.plan.filter((m) => m.disposition === "remove").length;
                notify(
                    res.onlinePending
                        ? `离线层判定剔除 ${remove} 项 · 联网反查进行中`
                        : `已重新自动分类：剔除 ${remove} · 保留 ${res.plan.length - remove}`,
                    "success"
                );
            }
        } catch {
            notify("自动分类失败，当前方案保持不变", "error");
            setClassifying(false);
        }
    }

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
        void runClassify(false);
        void api.listMcVersions().then((l) => setMcOptions(l.map(toOption)));
        void api.listJavaVersions().then((l) => setJavaOptions(l.map(toOption)));
        void api.getSettings().then(setSettings);
    }, [manifest]);

    // 在线反查的补全结论：后端换包后会停推，这里再按 fileName 拦一道，防迟到事件串台
    const packName = manifest?.fileName;
    useEffect(() => {
        let alive = true;
        let off: (() => void) | null = null;
        void api
            .onClassified((e) => {
                if (!alive) return;
                if (e.fileName && packName && e.fileName !== packName) return;
                setPlan(e.plan);
                // 离线那次推送只是先给结论，本轮结束（done）才停「分类中」
                if (!e.done) return;
                setClassifying(false);
                if (!e.complete) notify("联网反查未全部完成，剩余行沿用离线结论", "warn");
            })
            .then((f) => {
                if (alive) off = f;
                else f();
            });
        return () => {
            alive = false;
            off?.();
        };
    }, [packName]);

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

    /** 当前展示方案：原始方案 + 本页新增（同 id 以新增行为准，避免双行），套用处置与停用标记 */
    const mods = useMemo(() => {
        const extraIds = new Set(extras.map((m) => m.id));
        return [...plan.filter((m) => !extraIds.has(m.id)), ...extras].map((m) => {
            const disposition = overrides[m.id] ?? m.disposition;
            const disabled = disposition === "add" && disabledIds.has(m.id);
            return { ...m, disposition, ...(disabled ? { disabled: true } : {}) };
        });
    }, [plan, extras, overrides, disabledIds]);

    /** 参与构建的行（停用行除外）：计数、预下载聚合、下发后端的方案都用它 */
    const activeMods = useMemo(() => mods.filter((m) => !m.disabled), [mods]);

    const counts = useMemo(
        () => ({
            remove: activeMods.filter((m) => m.disposition === "remove").length,
            keep: activeMods.filter((m) => m.disposition === "keep").length,
            // add = 生效新增数（停用不计）；addTotal = 清单行数（弹窗「查看全部」口径）
            add: activeMods.filter((m) => m.disposition === "add").length,
            addTotal: mods.filter((m) => m.disposition === "add").length,
        }),
        [activeMods, mods]
    );

    /** 无任何端证据的行：默认保留（多留不炸服、误删才会），只在卡底给一句汇总，不逐行标噪音 */
    const unresolved = useMemo(
        () => plan.filter((m) => (m.envSource ?? "unknown") === "unknown").length,
        [plan]
    );

    /** 本地兜底聚合（后端答不上来时展示）：联网行按源 fileSize 求和 */
    const localEstimate = useMemo<DownloadEstimate>(() => {
        let downloadBytes = 0;
        let fromPackBytes = 0;
        for (const m of activeMods) {
            if (m.disposition === "remove") continue;
            if (m.needsDownload) downloadBytes += m.sizeBytes ?? 0;
            else fromPackBytes += m.sizeBytes ?? 0;
        }
        // 加载器本体：Fabric 一体化 server jar 约 25MB，Forge/NeoForge installer 约 12MB
        downloadBytes += manifest?.loader === "fabric" ? 25_000_000 : 12_000_000;
        return { downloadBytes, fromPackBytes, complete: false };
    }, [activeMods, manifest]);

    /** 后端预估（estimate_download，与构建同源：缓存扣减 + HEAD 实测），到位前用 localEstimate */
    const [remoteEstimate, setRemoteEstimate] = useState<DownloadEstimate | null>(null);

    /** 预估指纹：影响下载分类/大小的字段全列入，任一变化即重新防抖请求 */
    const estimateKey = useMemo(() => {
        const rows = activeMods
            .filter((m) => m.disposition !== "remove")
            .map(
                (m) =>
                    `${m.id}|${m.disposition}|${m.sizeBytes ?? 0}|${m.needsDownload ? 1 : 0}|${m.localPath ?? ""}|${m.pinned?.url ?? ""}`
            )
            .join(";");
        return `${rows}#${options?.mcVersion}#${options?.loaderVersion}#${(options?.keepDirs ?? []).join(",")}`;
    }, [activeMods, options?.mcVersion, options?.loaderVersion, options?.keepDirs]);

    // 350ms 防抖向后端要真实预估；加载器版本未定时不发请求（构建期必失败，数字无意义）
    useEffect(() => {
        if (!options || !options.loaderVersion.trim()) {
            setRemoteEstimate(null);
            return;
        }
        let stale = false;
        const timer = setTimeout(() => {
            void api
                .estimateDownload(activeMods, options)
                .then((e) => !stale && setRemoteEstimate(e))
                .catch(() => {}); // 失败保留前值，回落不闪烁
        }, 350);
        return () => {
            stale = true;
            clearTimeout(timer);
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [estimateKey]);

    const estimate = remoteEstimate ?? localEstimate;

    /** 反向依赖警告：生效行依赖了被剔除或被停用的行（mrpack depends 元数据，按缺失项聚合） */
    const depWarnings = useMemo(() => {
        const byId = new Map(mods.map((m) => [m.id, m]));
        const groups = new Map<string, { missing: PlanMod; hosts: PlanMod[] }>();
        for (const m of activeMods) {
            if (m.disposition === "remove") continue;
            for (const d of m.depends ?? []) {
                const t = byId.get(d);
                if (t && (t.disposition === "remove" || t.disabled)) {
                    const g = groups.get(d) ?? { missing: t, hosts: [] };
                    g.hosts.push(m);
                    groups.set(d, g);
                }
            }
        }
        return [...groups.values()];
    }, [mods, activeMods]);

    /** 本地 .jar 添加的模组 id（徽章显示「本地」而非「推荐」） */
    const localIds = useMemo(
        () => new Set(extras.filter((m) => m.id.startsWith("local-")).map((m) => m.id)),
        [extras]
    );

    const setDisposition = (id: string, d: ModDisposition) =>
        setOverrides((o) => ({ ...o, [id]: d }));

    /** 手动改动数（处置覆写 + 停用行）：>0 才露出「清空我的修改」出口 */
    const manualEdits = Object.keys(overrides).length + disabledIds.size;

    /** 清空手动改动 = 回到自动分类结果；两段式确认，第二次点击才执行 */
    const clearEdits = () => {
        setOverrides({});
        setDisabledIds(new Set());
        setConfirmClear(false);
        notify("已清空手动修改，方案回到自动分类结果", "success");
    };

    /** 卡片行内移除单个保留目录（批量增删走 DirPickerModal 应用回写） */
    const removeDir = (name: string) => {
        patch({ keepDirs: (options?.keepDirs ?? []).filter((d) => d !== name) });
    };

    /** 新增行勾选 = 是否生效：取消勾选只停用（行保留在清单），自动补齐项停用另给全局警告 */
    const toggleAddActive = (m: PlanMod) => {
        const disable = !m.disabled;
        setDisabledIds((s) => {
            const next = new Set(s);
            if (disable) next.add(m.id);
            else next.delete(m.id);
            return next;
        });
        if (disable && m.autoSupplement) {
            notify(
                `已停用 ${m.name}：服务端必需前置，缺失可能导致依赖它的模组失效`,
                "warn"
            );
        }
    };

    /** 显式删除仅限用户自行添加的行（extras）；系统补行只能停用不能抹掉。
     *  删除刻意不发全局提示：高频操作会刷屏，行消失本身就是反馈 */
    const removeRow = (m: PlanMod) => {
        setExtras((e) => e.filter((x) => x.id !== m.id));
        setDisabledIds((s) => {
            if (!s.has(m.id)) return s;
            const next = new Set(s);
            next.delete(m.id);
            return next;
        });
    };

    /** 本页新增行的 id 集合（区分用户添加行与系统补行，决定 × 是否出现） */
    const extrasIds = useMemo(() => new Set(extras.map((m) => m.id)), [extras]);

    const addLocal = async () => {
        const path = await api.pickJarFile();
        if (!path) return;
        const fileName = path.split(/[\\/]/).pop() ?? path;
        const id = `local-${fileName}`;
        const row: PlanMod = {
            id,
            name: fileName.replace(/\.jar$/i, ""),
            version: fileName,
            disposition: "add",
            clientOnly: false,
            needsReview: false,
            autoSupplement: false,
            needsDownload: false,
            localPath: path,
        };
        // 同一 jar 再次添加 = 就地覆盖并复活（停用行重新生效）
        setExtras((e) => {
            const i = e.findIndex((m) => m.id === id);
            if (i === -1) return [...e, row];
            const next = [...e];
            next[i] = row;
            return next;
        });
        setDisabledIds((s) => {
            if (!s.has(id)) return s;
            const next = new Set(s);
            next.delete(id);
            return next;
        });
        setTab("add");
    };

    /** 在线添加：选中某个构建版本后回写新增列表；同模组再次添加 = 就地换版本（mods/ 不允许双版本并存） */
    const addOnline = (mod: ModSearchResult, version: ModVersionEntry) => {
        // 钉住用户此刻所选构建：构建时按此下载，版本与所选严格一致
        const row: PlanMod = {
            id: mod.id,
            name: mod.name,
            version: version.versionNumber,
            loader: loaderLabel(version.loader),
            disposition: "add",
            clientOnly: false,
            needsReview: false,
            autoSupplement: false,
            sizeBytes: version.sizeBytes,
            needsDownload: true,
            pinned: {
                url: version.url,
                sha1: version.sha1,
                fileName: version.fileName,
            },
        };
        const replaced = extras.find((m) => m.id === mod.id);
        if (replaced) {
            setExtras((e) => e.map((m) => (m.id === mod.id ? row : m)));
            // 版本确有变化才提示；覆盖发生在弹窗内，反馈走侧栏底部全局提示区
            if (replaced.version !== version.versionNumber) {
                notify(`已将 ${mod.name} 的构建换为 ${version.versionNumber}`, "success");
            }
        } else {
            setExtras((e) => [...e, row]);
        }
        // 再次添加视为重新启用：清掉该行的停用标记与处置覆写（包内同名行被剔除过也能复活）
        setDisabledIds((s) => {
            if (!s.has(mod.id)) return s;
            const next = new Set(s);
            next.delete(mod.id);
            return next;
        });
        setOverrides((o) => {
            if (!(mod.id in o)) return o;
            const next = { ...o };
            delete next[mod.id];
            return next;
        });
        setTab("add");
    };

    const start = async () => {
        if (!manifest || !options || starting) return;
        setStarting(true);
        try {
            // 停用行不下发：后端方案里根本没有它，无需感知停用概念
            const { taskId, queued } = await api.startConversion(options, manifest, activeMods);
            // 同一时间只跑一条转换：有任务在跑时本次进排队队列
            if (queued) notify("已有转换正在进行，本次任务已加入队列", "info");
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
                        <Panel gap={14} className="h-[260px]">
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

                            {/* 行区：行区顶部铺开、出口 mt-auto 钉在卡底（下方只剩 p-5 的 20 内边距）；
                                内容高低差产生的余量全部落在「列表 ↔ 出口」之间，切页签/出警告时出口不跳位 */}
                            <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-hidden">
                                <AnimatePresence mode="wait" initial={false}>
                                    <motion.div
                                        key={tab}
                                        initial={{ opacity: 0, y: 10 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -10 }}
                                        transition={{ duration: 0.18, ease: "easeOut" }}
                                        className="flex min-h-0 shrink flex-col gap-2.5 overflow-hidden"
                                    >
                                        {rows.length === 0 ? (
                                            <p className="flex h-[140px] w-full items-center justify-center text-[11px] text-text-3">
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
                                                            ? toggleAddActive(m)
                                                            : setDisposition(
                                                                  m.id,
                                                                  m.disposition === "remove"
                                                                      ? "keep"
                                                                      : "remove"
                                                              )
                                                    }
                                                    onRemove={
                                                        m.disposition === "add" &&
                                                        extrasIds.has(m.id)
                                                            ? () => removeRow(m)
                                                            : undefined
                                                    }
                                                />
                                            ))
                                        )}
                                    </motion.div>
                                </AnimatePresence>

                            {/* 反向依赖警告：保留/生效新增行依赖了被剔除或被停用的模组，一键恢复即消警 */}
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
                                                    {hosts.length > 2 ? ` 等 ${hosts.length} 项` : ""} 依赖被
                                                    {missing.disabled ? "停用" : "剔除"}的 {missing.name}
                                                </span>
                                                <LinkBtn
                                                    size="sm"
                                                    onClick={() =>
                                                        missing.disabled
                                                            ? toggleAddActive(missing)
                                                            : setDisposition(missing.id, "keep")
                                                    }
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

                            {/* 出口区（行区内、钉在卡底）：剔除/保留态取链接自然高（~18），
                                新增态=查看链接居左、两枚 h32 添加按钮居右；上下不虚占固定高 */}
                            <div className="mt-auto flex shrink-0 flex-col">
                                <AnimatePresence mode="wait" initial={false}>
                                    <motion.div
                                        key={tab}
                                        initial={{ opacity: 0, y: 8 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -6 }}
                                        transition={{ duration: 0.18, ease: "easeOut" }}
                                        className="flex w-full items-center justify-between"
                                    >
                                        {(tab === "remove" || tab === "keep") && (
                                            <>
                                                <LinkBtn chevron onClick={() => setListFocus(tab)}>
                                                    查看全部 {tab === "remove" ? counts.remove : counts.keep} 项
                                                    {tab === "remove" ? "剔除" : "保留"}清单
                                                </LinkBtn>
                                                {/* 自动分类出口：进页已默认跑过，这里只给重跑与回退手动改动的入口；
                                                    无证据行数以一行汇总提示，不逐行标「待确认」 */}
                                                <div className="flex min-w-0 items-center gap-2.5">
                                                    {unresolved > 0 && (
                                                        <span className="truncate text-[10px] leading-[14px] text-text-3">
                                                            {unresolved} 项无依据 · 默认保留
                                                        </span>
                                                    )}
                                                    {classifying ? (
                                                        <span className="text-[11px] leading-[16px] text-text-3">
                                                            自动分类中…
                                                        </span>
                                                    ) : (
                                                        <>
                                                            <LinkBtn size="sm" onClick={() => void runClassify(true)}>
                                                                重新自动分类
                                                            </LinkBtn>
                                                            {manualEdits > 0 &&
                                                                (confirmClear ? (
                                                                    <>
                                                                        <LinkBtn
                                                                            size="sm"
                                                                            className="text-redstone"
                                                                            onClick={clearEdits}
                                                                        >
                                                                            确认清空 {manualEdits} 项
                                                                        </LinkBtn>
                                                                        <LinkBtn
                                                                            size="sm"
                                                                            className="text-text-3"
                                                                            onClick={() => setConfirmClear(false)}
                                                                        >
                                                                            取消
                                                                        </LinkBtn>
                                                                    </>
                                                                ) : (
                                                                    <LinkBtn
                                                                        size="sm"
                                                                        className="text-text-2"
                                                                        onClick={() => setConfirmClear(true)}
                                                                    >
                                                                        清空我的修改
                                                                    </LinkBtn>
                                                                ))}
                                                        </>
                                                    )}
                                                </div>
                                            </>
                                        )}
                                        {tab === "add" && (
                                            <>
                                                {counts.addTotal > 0 && (
                                                    <LinkBtn chevron onClick={() => setListFocus("add")}>
                                                        查看全部 {counts.addTotal} 项新增清单
                                                    </LinkBtn>
                                                )}
                                                {/* 两枚 h32 添加按钮：有清单时居右，空方案时整行居中 */}
                                                <div
                                                    className={cn(
                                                        "flex items-center gap-2.5",
                                                        counts.addTotal === 0 &&
                                                            "w-full justify-center"
                                                    )}
                                                >
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
                                            </>
                                        )}
                                    </motion.div>
                                </AnimatePresence>
                            </div>
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
                                勾选的目录按原层级从源包复制到服务端（支持子目录）
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
                            {estimate.downloadBytes > 0
                                ? `预计下载 ${formatSize(estimate.downloadBytes)}`
                                : "无需联网下载 · 全部来自整合包与本地"}
                            {!estimate.complete && estimate.downloadBytes > 0 && "（估算）"}
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
                // 三窗各列本处置的行（窗内全部展示，滚动）
                mods={mods.filter((m) => m.disposition === (listFocus ?? "remove"))}
                onDisposition={(id, d) => {
                    // 新增清单里取消勾选 = 停用该行（与卡片行内取消同语义；勾选回来即恢复）
                    const row = mods.find((m) => m.id === id);
                    if (row && row.disposition === "add") toggleAddActive(row);
                    else setDisposition(id, d);
                }}
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

/* ---------------- 方案行（mod-row）：勾选框 16 + 名称/版本横排 + 右侧徽章（可删行悬停露出 ×） ---------------- */

function PlanModRow({
    mod,
    badge,
    onToggle,
    onRemove,
}: {
    mod: PlanMod;
    badge?: React.ReactNode;
    onToggle: () => void;
    /** 仅用户自行添加的新增行提供显式删除；缺省 = 不渲染 × */
    onRemove?: () => void;
}) {
    // 勾选语义：剔除项未勾选；新增行取消勾选 = 停用（行保留），整行压暗表意不参与构建
    const included = mod.disposition !== "remove" && !mod.disabled;

    return (
        <div className={cn("group flex w-full items-center gap-2.5", mod.disabled && "opacity-55")}>
            <CheckBox checked={included} review={!included && mod.needsReview} onChange={onToggle} />
            <div className="flex min-w-0 flex-1 items-center gap-2">
                <span
                    className={cn(
                        "truncate font-mono text-[12px] leading-[18px] font-medium",
                        mod.disabled ? "text-text-3" : "text-text-1"
                    )}
                >
                    {mod.name}
                </span>
                <span className="truncate font-mono text-[11px] leading-[16px] font-normal text-text-3">
                    {mod.version}
                    {mod.loader ? ` · ${mod.loader}` : ""}
                </span>
            </div>
            {badge}
            {onRemove && (
                <button
                    onClick={onRemove}
                    title="从方案移除"
                    className={cn(
                        "flex size-6 shrink-0 items-center justify-center rounded-md text-text-3",
                        "opacity-0 transition-[opacity,color,background-color] duration-150",
                        "hover:bg-redstone-dim hover:text-redstone group-hover:opacity-100 focus-visible:opacity-100"
                    )}
                >
                    <X className="size-3" />
                </button>
            )}
        </div>
    );
}

/** 徽章优先级：自动补齐 > 需人工确认 > 本地 > 客户端专属（附判定依据，剔除是破坏性操作，必须说清凭什么） */
function badgeFor(mod: PlanMod, local: boolean): React.ReactNode {
    if (mod.autoSupplement) return <TagChip>自动补齐</TagChip>;
    if (mod.needsReview) return <ToneChip tone="gold" size="sm">需人工确认</ToneChip>;
    if (local) return <TagChip>本地</TagChip>;
    if (mod.clientOnly)
        return <TagChip>{`客户端专属 · ${evidenceLabel(mod.envSource)}`}</TagChip>;
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

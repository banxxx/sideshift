/**
 * 转换配置页 Convert（SS.pen `dEsbp` 剔除态 / `pRF47` 新增态 / `sc3I9` 下拉展开态）
 *
 * checkout 式骨架：BodyRow gap20 = 左列（gap16，三张卡各 gap14）+ 右栏 280px 摘要卡。
 * 全局默认值来自设置页（api.defaultOptions），本页做的是“单包覆写”——离开即丢弃。
 *
 * 这一页只做一件事：**配置一次新的转换**。回看某个任务真正用过的方案是任务详情「方案」签的活
 * （见 PlanReviewView），它读的是任务存档；这里的装载链全部围着「刚选完的包」现算。
 *
 * 模组方案的处置编辑模型：
 *  - plan（后端/mock 给的原始方案）+ extras（本页新增的模组）为数据源
 *  - overrides 记录用户对 remove/keep 的改动；disabledIds 记录被停用的新增行（行保留、不构建）
 *  - 计数/摘要/下发后端的方案一律由 plan+extras+overrides+disabledIds 派生，保证口径不漂移
 *
 * 本文件只留「状态 → 派生 → 动作 → 右栏摘要」这条数据主干：四张设置卡见 OptionCards，
 * 模组方案卡见 ModPlanCard，方案行与徽章见 PlanModRow，静态选项见 constants。
 */
import { Archive, ChevronRight, Download, Folder, Layers } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { useNavigation } from "@/lib/navigation";
import { notify } from "@/lib/notify";
import { formatSize, loaderLabel, outputNameOf, reviewFirst, truncateMiddle } from "@/lib/format";
import type {
    AppSettings,
    ConversionOptions,
    DownloadEstimate,
    EnvSource,
    ModDisposition,
    ModSearchResult,
    ModVersionEntry,
    PackDirNode,
    PackManifest,
    PlanMod,
} from "@/lib/types";
import {
    Btn,
    CountRow,
    Divider,
    LinkBtn,
    NoteRow,
    PageHeader,
    Panel,
    PanelHead,
    type SelectOption,
} from "@/components/ui";
import { DirPickerModal, OnlineAddModal, PlanListModal, type ListFocus } from "@/features/convert/modals";
import { LaunchArgsCard, KeepDirsCard, RuntimeEnvCard, ServerSettingsCard } from "./OptionCards";
import { ModPlanCard } from "./ModPlanCard";
import { CARD_RISE, KEEP_DIR_PRESETS, PAGE_RISE, PREVIEW_ROWS, toOption } from "./constants";

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
    /** 「全部清单」弹窗：视角与开关分两个状态。
     *  关闭只翻 listOpen——视角一旦跟着清空，退场那 200ms（base-ui 在 data-[ending-style] 期间仍挂着
     *  Popup）focus 会退回默认「剔除」，人就看到清单突然换成剔除的数据再消失。 */
    const [listOpen, setListOpen] = useState(false);
    const [listFocus, setListFocus] = useState<ListFocus>("remove");
    const openList = (f: ListFocus) => {
        setListFocus(f);
        setListOpen(true);
    };
    const [onlineOpen, setOnlineOpen] = useState(false);
    const [starting, setStarting] = useState(false);
    /** 全局设置：摘要卡展示默认输出目录（本次覆写为空时回落它） */
    const [settings, setSettings] = useState<AppSettings | null>(null);
    /** 包内可保留目录树（目录勾选弹窗数据源） */
    const [packDirs, setPackDirs] = useState<PackDirNode[]>([]);
    const [dirModalOpen, setDirModalOpen] = useState(false);
    /** 自动分类进行中：离线层是同步返回，在线层补全后走 classified 事件再刷一次 */
    const [classifying, setClassifying] = useState(false);
    /** 手动重跑的那一小段：列表已被清空、离线结论还没回来（只用来挑文案） */
    const [reclassifying, setReclassifying] = useState(false);
    /** 「清空我的修改」两段式确认（弹窗纪律：不用遮罩/确认框，第二次点击才执行） */
    const [confirmClear, setConfirmClear] = useState(false);

    /** 自动分类主入口：进页默认执行，「重新自动分类」手动再跑一次。
     *  手动处置存在 overrides，方案整体替换也不会覆盖用户改动。
     *  命令返回只代表离线层跑完；联网反查在后台补全，收尾靠 classified 事件。
     *  手动重跑先清空方案：不清的话第二轮只要给出同样的处置，行的 id 集合就没变过，
     *  既不出场也不入场 → 读起来是「整屏一起刷新」，逐行插入的节拍出不来（失败时回滚，别把列表清丢）。 */
    async function runClassify(manual: boolean) {
        const prevPlan = plan;
        setClassifying(true);
        if (manual) {
            setReclassifying(true);
            setPlan([]);
        }
        try {
            const res = await api.classifyPack();
            setReclassifying(false);
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
            setReclassifying(false);
            if (manual) setPlan(prevPlan);
            notify("自动分类失败，当前方案保持不变", "error");
            setClassifying(false);
        }
    }

    // 数据装载：包正躺在后端解析缓存里，按全局默认值 + 自动分类现算一份待确认的方案。
    // `defaultOptions`/`classifyPack`/`listPackDirs` 都只认「最近一次解析的包」，所以这条链只能服务
    // 刚选完的包；回看某个任务真正用过的方案走任务详情「方案」签（读存档，不碰这三个命令）。
    useEffect(() => {
        if (!manifest) return;
        void Promise.all([api.defaultOptions(manifest), api.listPackDirs()]).then(
            ([o, nodes]) => {
                setPackDirs(nodes);
                const present = KEEP_DIR_PRESETS.filter((name) =>
                    nodes.some((n) => n.name.toLowerCase() === name)
                );
                setOptions({
                    ...o,
                    mcVersion: manifest.mcVersion,
                    keepDirs: o.keepDirs.length ? o.keepDirs : present,
                });
            }
        );
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

    /** 分类期间的「待定」行 = 服务端轴还没有端证据、用户也没手动处置过的行。
     *  离线层面对 Forge 包（mods.toml 无端字段）和打包者乱填 env 的 mrpack，整包都答不上，
     *  旧口径会把它们全渲染成「剔除 + 需人工确认」，联网结论到达后再成批翻回保留 = 一闪而过的假结论。
     *  现在待定行不进任何计数、不进清单，只由卡底的分类进度交代；serverSide 一有值就自动落地。 */
    const pendingIds = useMemo(() => {
        if (!classifying) return new Set<string>();
        return new Set(
            mods.filter(
                (m) =>
                    m.disposition !== "add" &&
                    m.serverSide === undefined &&
                    (m.envSource ?? "unknown") === "unknown" &&
                    !(m.id in overrides)
            ).map((m) => m.id)
        );
    }, [classifying, mods, overrides]);

    /** 已判定行：页签计数 / 预览行 / 依赖警告只看这一批 */
    const settledMods = useMemo(
        () => activeMods.filter((m) => !pendingIds.has(m.id)),
        [activeMods, pendingIds]
    );

    /** 卡内预览行（页签里可见的最多 5 行）：待人工确认的行置顶（与「全部清单」弹窗同一口径），待定行不出现。
     *  提到 hooks 区计算，让模组方案卡那套入场节拍只盯着「谁进了可见列表」。 */
    const previewRows = useMemo(
        () =>
            reviewFirst(
                mods.filter((m) => m.disposition === tab && !pendingIds.has(m.id))
            ).slice(0, PREVIEW_ROWS),
        [mods, tab, pendingIds]
    );

    const counts = useMemo(
        () => ({
            remove: settledMods.filter((m) => m.disposition === "remove").length,
            keep: settledMods.filter((m) => m.disposition === "keep").length,
            // add = 生效新增数（停用不计）；addTotal = 清单行数（弹窗「查看全部」口径）
            add: settledMods.filter((m) => m.disposition === "add").length,
            addTotal: mods.filter((m) => m.disposition === "add").length,
        }),
        [settledMods, mods]
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

    /** 反向依赖警告：生效行依赖了被剔除或被停用的行（mrpack depends 元数据，按缺失项聚合）。
     *  待定行不参与——它们只是还没判完，报「依赖被剔除的 X」是假告警 */
    const depWarnings = useMemo(() => {
        const byId = new Map(mods.map((m) => [m.id, m]));
        const groups = new Map<string, { missing: PlanMod; hosts: PlanMod[] }>();
        for (const m of settledMods) {
            if (m.disposition === "remove") continue;
            for (const d of m.depends ?? []) {
                const t = byId.get(d);
                if (t && !pendingIds.has(d) && (t.disposition === "remove" || t.disabled)) {
                    const g = groups.get(d) ?? { missing: t, hosts: [] };
                    g.hosts.push(m);
                    groups.set(d, g);
                }
            }
        }
        return [...groups.values()];
    }, [mods, settledMods, pendingIds]);

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
        // 端取证异步回填：行先落地（选完立刻看得见），阶梯跑完再补端标签与真实体积。
        // 行若在这期间被删掉，下面的 map 匹配不上即自然丢弃
        void api.inspectAddedMod(path).then((side) =>
            setExtras((e) =>
                e.map((m) =>
                    m.id === id
                        ? {
                              ...m,
                              clientSide: side.clientSide,
                              serverSide: side.serverSide,
                              envSource: side.envSource,
                              bytecodeHint: side.bytecodeHint,
                              sizeBytes: side.sizeBytes ?? m.sizeBytes,
                          }
                        : m
                )
            )
        );
    };

    /** 在线添加：选中某个构建版本后回写新增列表；同模组再次添加 = 就地换版本（mods/ 不允许双版本并存） */
    const addOnline = (mod: ModSearchResult, version: ModVersionEntry) => {
        // 端标签跟着用户所选的那一份构建走：构建级 environment 精确到文件，
        // 项目级 client_side/server_side 只在构建没给时兜底（依据文案也随之换口径）
        const buildSides = version.clientSide || version.serverSide;
        const sides = {
            clientSide: buildSides ? version.clientSide : mod.clientSide,
            serverSide: buildSides ? version.serverSide : mod.serverSide,
            envSource: (buildSides ? "modrinthHash" : "modrinthProject") as EnvSource,
        };
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
            ...sides,
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

    // 预览行：见上方 previewRows（口径唯一，节拍也按它算）
    const rows = previewRows;
    /** 方案是否已有行（分类首屏的空卡要说「正在读取整合包…」而不是「暂无模组」） */
    const totalRows = mods.length;
    /** 手动重跑会先清空方案，那一瞬不能说「正在读取整合包」（包早就读过了） */
    const readingLabel = reclassifying ? "正在重新自动分类…" : "正在读取整合包…";
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
                {/* 左列：运行环境 / 模组方案 / 保留目录 / 启动参数 / 服务端设置（卡间 16） */}
                <div className="flex min-w-0 flex-1 flex-col gap-4">
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <RuntimeEnvCard
                            options={options}
                            patch={patch}
                            manifest={manifest}
                            loader={loader}
                            mcOptions={mcOptions}
                            loaderOptions={loaderOptions}
                            javaOptions={javaOptions}
                        />
                    </motion.div>

                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <ModPlanCard
                            tab={tab}
                            onTab={setTab}
                            counts={counts}
                            rows={rows}
                            classifying={classifying}
                            totalRows={totalRows}
                            readingLabel={readingLabel}
                            depWarnings={depWarnings}
                            localIds={localIds}
                            removableIds={extrasIds}
                            manualEdits={manualEdits}
                            confirmClear={confirmClear}
                            onToggleRow={(m) =>
                                m.disposition === "add"
                                    ? toggleAddActive(m)
                                    : setDisposition(
                                          m.id,
                                          m.disposition === "remove" ? "keep" : "remove"
                                      )
                            }
                            onRemoveRow={removeRow}
                            onRestoreDep={(m) =>
                                m.disabled ? toggleAddActive(m) : setDisposition(m.id, "keep")
                            }
                            onOpenList={openList}
                            onReclassify={() => void runClassify(true)}
                            onConfirmClear={setConfirmClear}
                            onClearEdits={clearEdits}
                            onAddLocal={() => void addLocal()}
                            onAddOnline={() => setOnlineOpen(true)}
                        />
                    </motion.div>

                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <KeepDirsCard
                            options={options}
                            packDirs={packDirs}
                            onPick={() => setDirModalOpen(true)}
                            onRemove={removeDir}
                        />
                    </motion.div>

                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <LaunchArgsCard options={options} patch={patch} />
                    </motion.div>

                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <ServerSettingsCard options={options} patch={patch} />
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
                            {effectiveOutputDir
                                ? truncateMiddle(effectiveOutputDir, 26)
                                : "默认输出目录"}
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
                            disabled={
                                !options || starting || classifying || !options.loaderVersion.trim()
                            }
                            onClick={() => void start()}
                        >
                            {starting ? "创建任务中…" : "开始转换"}
                            {!starting && <ChevronRight className="size-[13px]" />}
                        </Btn>
                        <Btn size="sm" full className="font-medium" onClick={() => switchPrimary("home")}>
                            返回首页
                        </Btn>
                        <p className="w-full text-center text-[10px] leading-[14px] font-normal text-text-3">
                            {classifying
                                ? "自动分类进行中，方案落定后方可开始构建"
                                : options && !options.loaderVersion.trim()
                                  ? "正在获取 Loader 版本列表，选定后方可开始转换"
                                  : "转换过程可随时取消，已下载依赖自动缓存复用"}
                        </p>
                    </Panel>
                </motion.aside>
            </div>

            <PlanListModal
                open={listOpen}
                onClose={() => setListOpen(false)}
                focus={listFocus}
                // 三窗各列本处置的行（窗内全部展示，滚动）
                mods={mods.filter((m) => m.disposition === listFocus)}
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

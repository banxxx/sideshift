/**
 * 转换配置页：只做「配置一次新的转换」（回看用过的方案是任务详情「方案」签的活，见 PlanReviewView）。
 * checkout 式骨架：左列三张设置卡 + 右栏 280px 摘要卡；全局默认值来自设置页，本页是单包覆写——只活在这一次选包里（草稿由 pack-store 接回，换包/改影响判定的全局设置即作废）。
 * 方案编辑模型：plan+extras 为数据源，overrides 记录 remove/keep 改动，disabledIds 记录停用的新增行；计数/摘要/下发后端的方案一律由这四者派生，保证口径不漂移。
 * 本文件只留「状态 → 派生 → 动作 → 右栏摘要」数据主干：设置卡见 OptionCards，方案卡见 ModPlanCard，方案行见 PlanModRow，静态选项见 constants。
 */
import { Archive, ChevronRight, Download, Folder, Layers, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { usePackStore, type PlanDraft } from "@/lib/pack-store";
import { useNavigation } from "@/lib/navigation";
import { notify } from "@/lib/notify";
import { formatSize, loaderLabel, outputNameOf, reviewFirst, truncateMiddle } from "@/lib/format";
import { useT } from "@/lib/i18n";
import type {
    AppSettings,
    ConversionOptions,
    DownloadEstimate,
    EnvSource,
    JavaProbe,
    ModDisposition,
    ModSearchResult,
    ModVersionEntry,
    PackDirTree,
    PackManifest,
    PlanMod,
} from "@/lib/types";
import { SERVER_PORT_RANGE, inRange } from "@/lib/types";
import {
    Btn,
    CheckBox,
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
import { TemplateMiniCard } from "@/features/templates/TemplateMiniCard";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { PREVIEW_ROWS, toOption } from "./constants";

export function ConvertPage() {
    const t = useT();
    const { entry, navigate, switchPrimary } = useNavigation();
    const manifest = entry.params?.manifest as PackManifest | undefined;
    const { getDraft, saveDraft } = usePackStore();
    /** 这一套选包的方案草稿：只在这次挂载的最初一帧取一次（取不到就是首次进来）。
     *  本页只挂导航栈栈顶，回首页即卸载 ⇒ 没有草稿就得整套重跑一遍自动分类。 */
    const [draft] = useState(getDraft);

    const [options, setOptions] = useState<ConversionOptions | null>(null);
    const [plan, setPlan] = useState<PlanMod[]>(() => draft?.plan ?? []);
    const [extras, setExtras] = useState<PlanMod[]>(() => draft?.extras ?? []);
    const [overrides, setOverrides] = useState<Record<string, ModDisposition>>(
        () => draft?.overrides ?? {}
    );
    /** 被停用的新增行 id：取消勾选=停用（行保留在清单，不参与构建），替代旧的「移除+撤销」链路 */
    const [disabledIds, setDisabledIds] = useState<Set<string>>(() => new Set(draft?.disabledIds));
    const [tab, setTab] = useState<ModDisposition>("remove");
    const [mcOptions, setMcOptions] = useState<SelectOption[]>([]);
    const [loaderOptions, setLoaderOptions] = useState<SelectOption[]>([]);
    /** 已经为哪一档 MC 版本拉过加载器列表；与当前 `mcVersion` 不等 ⇒ 用户换过档，选中值要跟着重选 */
    const loaderMcRef = useRef<string | null>(null);
    /** 源包 `modrinth.index.json` 声明的加载器版本：换档后查不到可用列表时回归的默认选择 */
    const declaredLoaderRef = useRef<string>("");
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
    /** 包内可保留内容（目录树 + 根级散文件），勾选弹窗与卡片行内读数的数据源 */
    const [packTree, setPackTree] = useState<PackDirTree>({ dirs: [], files: [] });
    const [dirModalOpen, setDirModalOpen] = useState(false);
    /** 自动分类进行中：离线层是同步返回，在线层补全后走 classified 事件再刷一次。
     *  带草稿回来时它=「离开时联网还没跑完」，下面那条挂载 effect 会据此跟后端复核一次。 */
    const [classifying, setClassifying] = useState(draft?.onlinePending ?? false);
    /** 手动重跑的那一小段：列表已被清空、离线结论还没回来（只用来挑文案） */
    const [reclassifying, setReclassifying] = useState(false);
    /** 「清空我的修改」两段式确认（弹窗纪律：不用遮罩/确认框，第二次点击才执行） */
    const [confirmClear, setConfirmClear] = useState(false);
    /** 本机 JDK 探测结论（只在开了「本机安装 Loader」时探；null = 还没探或不需要） */
    const [javaProbe, setJavaProbe] = useState<JavaProbe | null>(null);

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
            // force = 「重新自动分类」按钮专用：不传的话后端认定这一包的端证据还在缓存里，
            // 直接现算方案返回（离线探测与联网轮都不跑），按钮就成了「重读上次结果」。
            // 自动那一趟（manual=false）保持复用——每次进页白付一遍逐 jar 扫描不值。
            const res = await api.classifyPack(manual);
            setReclassifying(false);
            setPlan(res.plan);
            setClassifying(res.onlinePending);
            // CF 官方导出的包只声明编号，jar 字节全在网上。名字可能早就被磁盘索引补好了（那之后
            // 补取零请求、屏上看着什么都不缺），但**取字节每一次都要现取直链**、没 Key 就是拒绝 ⇒
            // 闸门只看「这包有编号行 + 没配 Key」，热索引不许把缺 Key 藏到点转换才炸
            if (res.cfNeedsKey) {
                notify(
                    t("convert.cf-needs-key", "{{count}} 个模组的 jar 不在包里 · 构建要从 CurseForge 取，需要在设置里填 CurseForge API Key", {
                        count: res.cfRows,
                    }),
                    "warn"
                );
            }
            if (manual) {
                const remove = res.plan.filter((m) => m.disposition === "remove").length;
                // 联网还没跑完时不报数：那一批发出去会把「剔除 N 项」当成结论，可方案还没落定
                notify(
                    res.onlinePending
                        ? t("convert.offline-pass", "离线层判定完成 · 联网反查进行中")
                        : t("convert.reclassified-removed", "已重新自动分类：剔除 {{removed}} · 保留 {{kept}}", {
                              removed: remove,
                              kept: res.plan.length - remove,
                          }),
                    "success"
                );
            }
        } catch {
            setReclassifying(false);
            if (manual) setPlan(prevPlan);
            notify(t("convert.auto-classify", "自动分类失败，当前方案保持不变"), "error");
            setClassifying(false);
        }
    }

    // 数据装载：包正躺在后端解析缓存里，按全局默认值 + 自动分类现算一份待确认的方案。
    // `defaultOptions`/`classifyPack`/`listPackDirs` 都只认「最近一次解析的包」，所以这条链只能服务
    // 刚选完的包；回看某个任务真正用过的方案走任务详情「方案」签（读存档，不碰这三个命令）。
    // 自动分类只在「没有草稿」或「离开时联网还没跑完」时补一次：前者是首次进来确实没算过，
    // 后者要跟后端复核（联网结论可能在人不在这一页时才到）。草稿完整时整套清单已经在状态里，
    // 再跑一遍等于白付一次逐个开包内 jar 的离线探测 + 整屏重排（手动重跑另有「重新自动分类」按钮）。
    useEffect(() => {
        if (!manifest) return;
        void Promise.all([api.defaultOptions(manifest), api.listPackDirs()]).then(
            ([o, tree]) => {
                setPackTree(tree);
                // 保留条目不预勾任何一项：勾哪个保哪个是这张卡的语义（原先那批根级预勾已撤）
                setOptions({
                    ...o,
                    mcVersion: manifest.mcVersion,
                });
                // 记住「源包声明的那一档」与它属于哪个 MC 版本：下面加载器版本那条腿靠这两枚
                // 区分「首次装载」与「用户换档」——首次那档是 `modrinth.index.json` 给的，不该被动。
                declaredLoaderRef.current = o.loaderVersion;
                loaderMcRef.current = manifest.mcVersion;
            }
        );
        if (!draft || draft.onlinePending) void runClassify(false);
        void api.listMcVersions().then((l) => setMcOptions(l.map(toOption)));
        void api.getSettings().then(setSettings);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [manifest]);

    // Fabric 没有 installer 可跑 ⇒ 这一档对本包是空开关，探测也就没有读者（那颗下拉同时灰下来）
    const needsInstaller = !!manifest && manifest.loader !== "fabric";
    /** 「本机安装 Loader」开着时才需要探：一次 `java -version` 是一个几十毫秒的子进程 */
    const javaInstallOn = needsInstaller && !!options?.installLoaderLocally;
    // 探测的读者判据与那颗下拉的启用态是同一颗 `javaInstallOn`：列表候选就是这个回包给的，
    // 一边探一边不探就会看到"提示说没有 Java、下拉里却列着三枚"。
    // 需求线或手选那枚一变就重探、不看缓存：可预见的失败要在点转换**之前**说出来，
    // 而"本机现在到底有没有够格的 JDK"是会变的（用户装完回来就该变绿）。
    useEffect(() => {
        if (!javaInstallOn || !options?.javaVersion) {
            setJavaProbe(null);
            return;
        }
        // 每轮 effect 各持一个 alive：换档时上一轮的迟到回包会被丢掉，不会盖成新结论的假数据
        let alive = true;
        void api.probeJava(options.javaVersion, options.javaPath).then((p) => {
            if (alive) setJavaProbe(p);
        });
        return () => {
            alive = false;
        };
    }, [javaInstallOn, options?.javaVersion, options?.javaPath]);

    /** 点开下拉重探一次（卡片透传上来）：候选是本机实探出来的，装完 JDK 不改档位不会自己触发 */
    const reprobeJava = () => {
        if (!javaInstallOn || !options?.javaVersion) return;
        void api.probeJava(options.javaVersion, options.javaPath).then(setJavaProbe);
    };

    /**
     * Java 这一档拦下来的转换：只在"真的会去本机跑 installer"那一格分支里成立，且
     * **探测还在飞时不拦**（`javaProbe` 为 null）——结论没落地就把人挡住，等于让一个还不存在的判定
     * 决定按钮能不能按。拦的是两种：一枚都不够格、以及手选了枚低于需求线的。
     */
    const javaBlocked = javaInstallOn && javaProbe?.status === "fail";

    /**
     * 端口填歪了：越界或干脆清空（空串在 `NumField` 里按 0 提交，落在区间外）。
     * 判据与那一格的红字**同源**——两边读的都是 `options.serverPort` 与同一个 `SERVER_PORT_RANGE`，
     * 所以不会出现「染了红却还能按」或「按不动却不知为什么」。默认值 25565 永不触发。
     */
    const portBlocked = !!options && !inRange(options.serverPort, SERVER_PORT_RANGE);

    // 离开页面时把这一套方案交给 store（下次进来第一帧就是它）：值走 ref 传，
    // 免得每次勾选都惊动 App 级 context 让整棵页面树跟着重渲染。
    const liveDraftRef = useRef<PlanDraft | null>(null);
    useEffect(() => {
        liveDraftRef.current = { plan, extras, overrides, disabledIds, onlinePending: classifying };
    }, [plan, extras, overrides, disabledIds, classifying]);
    useEffect(
        () => () => {
            const d = liveDraftRef.current;
            // 空方案不存：开发期 StrictMode 的双挂载会立刻走一次 cleanup，那一次没有内容可留；
            // 存进去只会把「还没算完」当成草稿
            if (d && (d.plan.length > 0 || d.extras.length > 0)) saveDraft(d);
        },
        [saveDraft]
    );

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
                if (!e.complete) notify(t("convert.online-lookup", "联网反查未全部完成，剩余行沿用离线结论；点「重新自动分类」可再试"), "warn");
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

    // MC 版本变更 → 重新拉取该版本可用的加载器版本，并让**选中值**跟着这一档走。
    // 2026-09-28 实测两件事：Fabric 的 `versions/loader/{mc}` 对 1.13 以上各档给的是同一份全量表
    // （所以这一档的"变化"来自候选列表整份，而不是选中值自己挪位）；Forge/NeoForge 才是按 MC 分代
    // （1.20.1 是 47.x、1.21 是 2.x），旧号在新档里不存在 ⇒ 必须重选，否则会留在一个跑不通的号上，
    // 而 SearchSelect 找不到匹配项时是原样把字符串显示出来的，看上去就是"切了版本没反应"。
    const mcVersion = options?.mcVersion;
    useEffect(() => {
        if (!mcVersion) return;
        // 换档时上一轮的迟到回包不能盖成新档的列表（与 Java 探测那条腿同一个 alive 口径）
        let alive = true;
        // 首次装载那一档的版本号来自源包声明（`modrinth.index.json` 的 dependencies），属于「包里写的」
        // 而不是「我们选的」，不能在这里改写；只有用户换过 MC 版本才重选。
        const switched = loaderMcRef.current !== mcVersion;
        loaderMcRef.current = mcVersion;
        /** 选中值落到新档：用户的挑选在新档依然成立就不动，否则取推荐项（无推荐取首项）；
         *  列表空（那档查无可用版本，或请求失败）时回归源包声明那一档——他定的口径：拉不到数据就用默认。 */
        const settle = (opts: SelectOption[]) => {
            setLoaderOptions(opts);
            if (!switched) return;
            const rec = opts.find((o) => o.recommended) ?? opts[0];
            setOptions((o) => {
                if (!o || opts.some((x) => x.value === o.loaderVersion)) return o;
                return { ...o, loaderVersion: rec?.value ?? declaredLoaderRef.current };
            });
        };
        void api
            .listLoaderVersions(mcVersion)
            .then((l) => {
                if (alive) settle(l.map(toOption));
            })
            .catch(() => {
                if (alive) settle([]);
            });
        return () => {
            alive = false;
        };
    }, [mcVersion]);

    // 需求线也跟着 MC 版本走：不重取的话，提示里那句「本次需要 Java N 及以上」、「自动选择」挑哪一枚、
    // 实跑那把筛子用的都还是**源包**那一档（1.20.1 的包切到 1.21 仍然按 17 去跑 installer）。
    // 表只有后端一份（core::java::required_for_mc），前端不复刻；改写的是方案字段，
    // 因为 `javaVersion` 是快照的一部分，回看与重试读的都是当时那一档，不能到报告/实跑里现算。
    // 首帧 `defaultOptions` 已经算过一次 ⇒ 值相同不 patch，也就不会白重探一趟 Java。
    useEffect(() => {
        if (!mcVersion) return;
        let alive = true;
        void api.javaRequirement(mcVersion).then((line) => {
            if (alive && line)
                setOptions((o) =>
                    o && o.javaVersion !== line ? { ...o, javaVersion: line } : o
                );
        });
        return () => {
            alive = false;
        };
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

    /** 参与构建的行（停用行除外）：下发后端的方案与本地聚合都用它；展示口径见下面 `visibleMods` */
    const activeMods = useMemo(() => mods.filter((m) => !m.disabled), [mods]);

    /** 分类期间（离线那一趟 + 后台联网那一轮）一行都不给：页签计数、预览行、依赖警告、摘要计数
     *  全部为空，只由卡底那条 rail 交代进度。
     *  旧口径只拦「还没有端证据」的行，离线结论照旧先成批显示——Forge 包整包都是离线结论，
     *  联网落地时又成批改回去，那些行既是一次性假结论，又能被人当场改判，
     *  改完再被自动结论覆盖，等于自己跟自己打架。判定没跑完就没有结论可展示。
     *  只管「给不给人看」：构建载荷与下载预估仍吃上面的 activeMods——方案内容本身没变，
     *  只是还没到能确认的时机。 */
    const visibleMods = useMemo(() => (classifying ? [] : mods), [classifying, mods]);

    /** 展示用的生效行（停用行不计）：页签计数 / 依赖警告都只看这一批 */
    const visibleActive = useMemo(
        () => visibleMods.filter((m) => !m.disabled),
        [visibleMods]
    );

    /** 卡内预览行（页签里可见的最多 5 行）：待人工确认的行置顶（与「全部清单」弹窗同一口径）。
     *  提到 hooks 区计算，让模组方案卡那套入场节拍只盯着「谁进了可见列表」。 */
    const previewRows = useMemo(
        () => reviewFirst(visibleMods.filter((m) => m.disposition === tab)).slice(0, PREVIEW_ROWS),
        [visibleMods, tab]
    );

    const counts = useMemo(
        () => ({
            remove: visibleActive.filter((m) => m.disposition === "remove").length,
            keep: visibleActive.filter((m) => m.disposition === "keep").length,
            // add = 生效新增数（停用不计）；addTotal = 清单行数（弹窗「查看全部」口径）
            add: visibleActive.filter((m) => m.disposition === "add").length,
            addTotal: visibleMods.filter((m) => m.disposition === "add").length,
        }),
        [visibleActive, visibleMods]
    );

    /** CurseForge 探明「拿不到字节」的缺件行（结论由联网那一轮探链落在 `cfBlocked` 上）：
     *  剔除行本就不进产物，不计入。分两档按包内 `required` 声明——
     *  必选档拦住「开始转换」，要人显式勾选允许跳过才放行；可选档只报数，勾选框都不出。
     *  用 visibleActive：分类中途行还没落定，那时整套摘要都是空的，这一档也一样不抢跑。 */
    const missing = useMemo(() => {
        const rows = visibleActive.filter((m) => m.cfBlocked && m.disposition !== "remove");
        return {
            total: rows.length,
            required: rows.filter((m) => m.cfRequired).length,
            optional: rows.filter((m) => !m.cfRequired).length,
        };
    }, [visibleActive]);

    /** 必选缺件且没勾选跳过 = 拦。跟构建那道闸门（pipeline 阶段 3.0）同一判据，
     *  差别只在这里是「开始前就说」，那一处是兜底（回看快照、旧任务重试都可能绕过本页） */
    const missingBlocked = missing.required > 0 && !options?.allowMissingMods;

    /** 本地兜底聚合（后端答不上来时展示）：联网行按源 fileSize 求和 */
    const localEstimate = useMemo<DownloadEstimate>(() => {
        let downloadBytes = 0;
        let fromPackBytes = 0;
        for (const m of activeMods) {
            if (m.disposition === "remove") continue;
            // 与后端预估同口径：探明拿不到字节的行不进产物，也就不进取件量
            if (m.cfBlocked) continue;
            if (m.needsDownload) downloadBytes += m.sizeBytes ?? 0;
            else fromPackBytes += m.sizeBytes ?? 0;
        }
        // 加载器本体：Fabric 官方 server jar 约 25MB，Forge/NeoForge installer 约 12MB
        downloadBytes += manifest?.loader === "fabric" ? 25_000_000 : 12_000_000;
        return { downloadBytes, fromPackBytes, complete: false };
    }, [activeMods, manifest]);

    /** 后端预估（estimate_download，与构建同源：缓存扣减 + HEAD 实测），到位前用 localEstimate */
    const [remoteEstimate, setRemoteEstimate] = useState<DownloadEstimate | null>(null);

    /** 预估指纹：影响下载分类/大小的字段全列入，任一变化即重新防抖请求。
     *  钉住行既看 url（Modrinth 的永久链）也看 fileId（CurseForge 的链存档里没有，换构建只有 id 会变） */
    const estimateKey = useMemo(() => {
        const rows = activeMods
            .filter((m) => m.disposition !== "remove")
            .map(
                (m) =>
                    `${m.id}|${m.disposition}|${m.sizeBytes ?? 0}|${m.needsDownload ? 1 : 0}|${m.localPath ?? ""}|${m.pinned?.url ?? ""}|${m.pinned?.fileId ?? ""}`
            )
            .join(";");
        return `${rows}#${options?.mcVersion}#${options?.loaderVersion}#${(options?.keepDirs ?? []).join(",")}#${(options?.keepFiles ?? []).join(",")}`;
    }, [activeMods, options?.mcVersion, options?.loaderVersion, options?.keepDirs, options?.keepFiles]);

    // 350ms 防抖向后端要真实预估；加载器版本未定时不发请求（构建期必失败，数字无意义）。
    // MC 版本空（裸 zip 认不出来、用户还没选）同理不发：那一档既定不住模组兼容也定不住 Loader。
    // 分类中也不发：那批行马上会被联网结论改写，拿回来的数字是过期结论；
    // 落定时 classifying 跟着进依赖，这一趟才补上要的那一次。
    useEffect(() => {
        if (
            classifying ||
            !options ||
            !options.mcVersion.trim() ||
            !options.loaderVersion.trim()
        ) {
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
    }, [estimateKey, classifying]);

    const estimate = remoteEstimate ?? localEstimate;

    /** 反向依赖警告：生效行依赖了被剔除或被停用的行（mrpack depends 元数据，按缺失项聚合）。
     *  分类中整批不报（visible* 已经空了）：那时「被剔除的 X」多半只是离线结论，告的是假警 */
    const depWarnings = useMemo(() => {
        const byId = new Map(visibleMods.map((m) => [m.id, m]));
        const groups = new Map<string, { missing: PlanMod; hosts: PlanMod[] }>();
        for (const m of visibleActive) {
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
    }, [visibleMods, visibleActive]);

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
        notify(t("convert.manual-edits", "已清空手动修改，方案回到自动分类结果"), "success");
    };

    /** 卡片行内移除单个保留条目（批量增删走 DirPickerModal 应用回写）；目录与根级文件是两个字段，各删各的 */
    const removeDir = (name: string) => {
        patch({ keepDirs: (options?.keepDirs ?? []).filter((d) => d !== name) });
    };
    const removeFile = (name: string) => {
        patch({ keepFiles: (options?.keepFiles ?? []).filter((f) => f !== name) });
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
                t("convert.name-disabled", "已停用 {{name}}：服务端必需前置，缺失可能导致依赖它的模组失效", {
                    name: m.name,
                    // 模组名是数据：不关掉转义的话 `Alex's Mobs` 会显示成 `Alex&#39;s Mobs`
                    interpolation: { escapeValue: false },
                }),
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
            // 源给了多少就写多少：两侧支持度只有 Modrinth 声明，CurseForge 一家不给 ⇒
            // 这里先如实落「无依据」，行落地后由下面那趟按构建 sha1 补查，问到才换标签
            envSource: (
                mod.source === "curseforge"
                    ? "unknown"
                    : buildSides
                      ? "modrinthHash"
                      : "modrinthProject"
            ) as EnvSource,
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
                // CurseForge 的 url 是空串（时效签名链），构建时靠这两样现取
                source: mod.source,
                fileId: mod.source === "curseforge" ? version.id : undefined,
            },
        };
        const replaced = extras.find((m) => m.id === mod.id);
        if (replaced) {
            setExtras((e) => e.map((m) => (m.id === mod.id ? row : m)));
            // 版本确有变化才提示；覆盖发生在弹窗内，反馈走侧栏底部全局提示区
            if (replaced.version !== version.versionNumber) {
                notify(
                    t("convert.switched-name", "已将 {{name}} 的构建换为 {{version}}", {
                        name: mod.name,
                        version: version.versionNumber,
                        interpolation: { escapeValue: false },
                    }),
                    "success"
                );
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
        // 端补查：源本身没答上两侧时（CurseForge 一家不声明；Modrinth 偶发构建级与项目级都空），
        // 拿这份构建的 sha1 走与本地 jar 同一条取证阶梯——同一份 jar 两个平台哈希逐字相同，
        // 问到的是 Modrinth 的构建/项目层。行先落地、答上再补标签；没答上**保持原样**，
        // 别把已有的依据口径盖成没依据。用户在这期间换了构建 ⇒ sha1 对不上，本次结果直接丢弃
        const sha1 = version.sha1;
        if (!sides.clientSide && !sides.serverSide && sha1 && version.fileName) {
            void api
                .inspectAddedBuild(sha1, version.fileName, mod.name)
                .then((side) => {
                    if (side.envSource === "unknown") return;
                    setExtras((e) =>
                        e.map((m) =>
                            m.id === mod.id && m.pinned?.sha1 === sha1
                                ? {
                                      ...m,
                                      clientSide: side.clientSide,
                                      serverSide: side.serverSide,
                                      envSource: side.envSource,
                                  }
                                : m
                        )
                    );
                });
        }
    };

    const start = async () => {
        if (!manifest || !options || starting) return;
        setStarting(true);
        try {
            // 停用行不下发：后端方案里根本没有它，无需感知停用概念
            const { taskId, queued } = await api.startConversion(options, manifest, activeMods);
            // 同一时间只跑一条转换：有任务在跑时本次进排队队列
            if (queued) notify(t("convert.conversion-running", "已有转换正在进行，本次任务已加入队列"), "info");
            navigate("task", { taskId });
        } catch {
            setStarting(false);
        }
    };

    if (!manifest) {
        return (
            <div className="flex flex-col gap-5 py-6">
                <PageHeader title={t("convert.conversion-setup", "转换配置")} />
                <Panel className="items-center py-16">
                    <Layers className="size-6 text-text-3" />
                    <p className="text-[13px] text-text-2">{t("convert.modpack-selected", "还没有选择整合包，无法配置转换。")}</p>
                    <Btn
                        variant="primary"
                        size="sm"
                        className="mt-1"
                        onClick={() => switchPrimary("home")}
                    >
                        {t("convert.go-home", "返回首页选择整合包")}
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
    const readingLabel = reclassifying ? t("convert.reclassifying", "正在重新自动分类…") : t("convert.reading-modpack", "正在读取整合包…");
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
            className="flex flex-col gap-5 overflow-hidden py-6"
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
        >
            <motion.div variants={CARD_RISE}>
                <PageHeader
                    title={t("convert.conversion-setup", "转换配置")}
                    // 前半截全是数据（文件名 / Loader / 版本号），不过 t：
                    // i18next 默认转义插值值，文件名里的 `&` `'` `/` 会被写成实体
                    sub={`${truncateMiddle(manifest.fileName, 34)} · ${loader} · Minecraft ${
                        // 裸 zip 认不出版本号时后端给空串：这里如实说「未识别」，不再显示一个凭空造的档
                        manifest.mcVersion || t("convert.unrecognized", "未识别")
                    } · ${t("convert.detection-finished", "检测完成，确认转换方案后开始构建")}`}
                />
            </motion.div>

            <div className="flex items-start gap-5">
                {/* 左列：运行环境 / 模组方案 / 保留内容 / 启动参数 / 服务端设置（卡间 16） */}
                <div className="flex min-w-0 flex-1 flex-col gap-4">
                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <RuntimeEnvCard
                            options={options}
                            patch={patch}
                            manifest={manifest}
                            loader={loader}
                            mcOptions={mcOptions}
                            loaderOptions={loaderOptions}
                            javaProbe={javaProbe}
                            onJavaProbe={reprobeJava}
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
                            packTree={packTree}
                            onPick={() => setDirModalOpen(true)}
                            onRemove={removeDir}
                            onRemoveFile={removeFile}
                        />
                    </motion.div>

                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <LaunchArgsCard options={options} patch={patch} />
                    </motion.div>

                    <motion.div variants={CARD_RISE} className="min-w-0">
                        <ServerSettingsCard options={options} patch={patch} />
                    </motion.div>
                </div>

                {/* 右栏：配置模板小卡 + 转换摘要（280px 固定宽） */}
                <motion.aside variants={CARD_RISE} className="flex w-[280px] shrink-0 flex-col gap-4">
                    <TemplateMiniCard options={options} patch={patch} />
                    <Panel gap={14}>
                        <PanelHead title={t("convert.conversion-summary", "转换摘要")} />
                        <CountRow label={t("convert.client-mods", "剔除客户端模组")} count={counts.remove} tone="gold" />
                        <CountRow label={t("convert.server-mods", "保留服务端模组")} count={counts.keep} tone="emerald" />
                        <CountRow label={t("convert.server-mods-added", "新增服务端模组")} count={counts.add} tone="accent" />
                        <Divider />
                        {/* 分类中这一行只说「还没算」：行没落定，下载量算出来也是个会被推翻的数 */}
                        <NoteRow icon={Download}>
                            {classifying
                                ? t("convert.estimating-wait", "分类落定后给出下载预估")
                                : estimate.downloadBytes > 0
                                  ? t("convert.estimated-download", "预计下载 {{size}}", { size: formatSize(estimate.downloadBytes) })
                                  : t("convert.downloads-needed", "无需联网下载 · 全部来自整合包与本地")}
                            {!classifying &&
                                !estimate.complete &&
                                estimate.downloadBytes > 0 &&
                                t("convert.estimated", "（估算）")}
                        </NoteRow>
                        <NoteRow icon={Archive}>
                            {t("convert.output", "输出")} {outputNameOf(manifest.fileName)}
                        </NoteRow>
                        <NoteRow icon={Folder}>
                            {effectiveOutputDir
                                ? truncateMiddle(effectiveOutputDir, 26)
                                : t("convert.default-output", "默认输出目录")}
                        </NoteRow>
                        <div className="flex w-full items-center justify-between gap-2">
                            <LinkBtn size="sm" onClick={() => void chooseOutputDir()}>
                                {outputOverride ? t("convert.change-folder", "更换本次目录…") : t("convert.use-another", "本次改用其他目录…")}
                            </LinkBtn>
                            {!!outputOverride && (
                                <LinkBtn size="sm" onClick={() => patch({ outputOverride: "" })}>
                                    {t("convert.restore-global", "恢复全局")}
                                </LinkBtn>
                            )}
                        </div>
                        {/* 缺件闸门：探测结论落在方案行上，这里在点转换之前就把「构建会少几个」说出来，
                            而不是等几十秒下载后在日志里撞 403。必选档要人显式勾选才放行。 */}
                        {missing.total > 0 && (
                            <>
                                <NoteRow icon={TriangleAlert} tone={missingBlocked ? "danger" : undefined}>
                                    {missing.required > 0
                                        ? t(
                                              "convert.missing-required",
                                              "{{total}} 个模组拿不到文件，其中必选 {{required}} 个",
                                              { total: missing.total, required: missing.required }
                                          )
                                        : t(
                                              "convert.missing-optional",
                                              "{{count}} 个可选模组拿不到文件，本次会跳过",
                                              { count: missing.optional }
                                          )}
                                </NoteRow>
                                {missing.required > 0 && (
                                    <div className="flex w-full items-start gap-2">
                                        <CheckBox
                                            checked={!!options?.allowMissingMods}
                                            onChange={(v) => patch({ allowMissingMods: v })}
                                        />
                                        <span className="min-w-0 flex-1 text-[11px] leading-[16px] font-normal text-text-1">
                                            {t("convert.allow-missing", "允许跳过拿不到文件的模组，其余照常构建")}
                                        </span>
                                    </div>
                                )}
                            </>
                        )}
                        <Btn
                            variant="primary"
                            full
                            disabled={
                                !options ||
                                starting ||
                                classifying ||
                                missingBlocked ||
                                javaBlocked ||
                                portBlocked ||
                                !options.mcVersion.trim() ||
                                !options.loaderVersion.trim()
                            }
                            onClick={() => void start()}
                        >
                            {starting ? t("convert.creating-task", "创建任务中…") : t("convert.start-conversion", "开始转换")}
                            {!starting && <ChevronRight className="size-[13px]" />}
                        </Btn>
                        <Btn size="sm" full className="font-medium" onClick={() => switchPrimary("home")}>
                            {t("convert.back-home", "返回首页")}
                        </Btn>
                        <p
                            className={`w-full text-center text-[10px] leading-[14px] font-normal ${
                                missingBlocked || javaBlocked || portBlocked ? "text-redstone" : "text-text-3"
                            }`}
                        >
                            {classifying
                                ? t("convert.auto-classifying-wait", "自动分类进行中，方案落定后方可开始构建")
                                : missingBlocked
                                  ? t(
                                        "convert.fix-missing",
                                        "有必选模组拿不到文件，先在方案里改成剔除，或勾选「允许跳过」"
                                    )
                                  : javaBlocked
                                    ? t("convert.fix-java", "请先在「运行环境」里处理好 Java，本次装不了 Loader")
                                    : portBlocked
                                      ? t("convert.enter-port", "请先在「服务端设置」里填一个 1–65535 的端口")
                                      : options && !options.mcVersion.trim()
                                        // MC 版本这档要排在 Loader 那句前面：版本空时加载器列表根本不会去拉，
                                        // 那句「正在获取 Loader 版本」会把人引向一个没在发生的过程
                                        ? t(
                                              "convert.pick-mc-version",
                                              "未识别到 Minecraft 版本，请先在「运行环境」里选择版本"
                                          )
                                        : options && !options.loaderVersion.trim()
                                          ? t("convert.fetching-loader", "正在获取 Loader 版本列表，选定后方可开始转换")
                                          : t("convert.cancel-anytime", "转换过程可随时取消，已下载依赖自动缓存复用")}
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
                tree={packTree}
                selected={options?.keepDirs ?? []}
                selectedFiles={options?.keepFiles ?? []}
                onApply={(dirs, files) => patch({ keepDirs: dirs, keepFiles: files })}
            />
        </motion.div>
    );
}

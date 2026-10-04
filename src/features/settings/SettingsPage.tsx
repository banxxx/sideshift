/**
 * 设置页：五个分组（转换选项 / 存储与缓存 / 下载与查询 / 端信息反查 / 外观与关于），每组 = 等宽小标题 + 一张无内边距卡片，行用 divide-y 分隔。
 * 读写走 @/lib/api 门面；主题走 @/lib/theme 单一真源（侧栏按钮同步）。
 * 设置是「改一处即持久化」：写盘失败必须外显并从后端重读，让界面与真正常量的那份一致。
 */
import { FlaskConical, Folder, Monitor, Moon, RefreshCw, Sun, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { channelLabel, formatSize, ratioPercent } from "@/lib/format";
import { notify, type NoticeKind } from "@/lib/notify";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { usePackStore } from "@/lib/pack-store";
import { switchTheme, useTheme, type Theme } from "@/lib/theme";
import { LOCALE_OPTIONS, LOCALE_TO_WIRE, applyLocaleChoice, systemLocale, t, useT, type AppLocale, type Locale, type TranslateFn } from "@/lib/i18n";
import { errOf } from "@/lib/errors";
import type {
    AppSettings,
    CacheUsage,
    CleanReport,
    UpdateChannel,
    UpdateStatus,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { openUpdate, runCheck, useUpdate } from "@/lib/update-store";
import {
    Btn,
    Collapse,
    IconBtn,
    ModalShell,
    PageHeader,
    Panel,
    SearchSelect,
    SectionTitle,
    SegTabs,
    SettingRow,
    Stepper,
    TextInput,
    Toggle,
    type SelectOption,
} from "@/components/ui";

/** 主题三态分段：`key` 是档位 id（不翻），`label` 过 `t`；表建在函数里，避免顶层求值冻在首帧语言上 */
function themeTabs(): Array<{ key: Theme; label: string; icon: typeof Sun }> {
    return [
        { key: "light", label: t("settings.light", "浅色"), icon: Sun },
        { key: "dark", label: t("common.dark", "深色"), icon: Moon },
        { key: "system", label: t("settings.system", "系统"), icon: Monitor },
    ];
}

type UpdateState = "idle" | "checking" | "latest" | "available" | "downloading" | "verifying" | "ready";

/** 取件那一轮的三个在飞/落定档位；不在档里（idle/failed/canceled）时由「查一次」的结论说话 */
function roundOf(stage: UpdateStatus["stage"]): UpdateState | null {
    return stage === "downloading" || stage === "verifying" || stage === "ready" ? stage : null;
}

/** 更新渠道两档：SegTabs 的 value 取解析后的档位，所以「跟随当前构建」在界面上不占第三态 */
function channelTabs(): Array<{ key: UpdateChannel; label: string }> {
    return [
        { key: "stable", label: t("settings.stable", "正式版") },
        { key: "beta", label: "Beta" },
    ];
}

/**
 * 这颗钮此刻说什么：那一轮 > 正在查 > 上一次查出来的结论 > 这趟还没查过。
 * 结论档会一直留着（旧版三秒后退回「检查更新」的那只定时器删了）：它说的是「本机知道的事」，
 * 而那份知道的事不会因为过了三秒就作废——钮现在同时是「回到那扇窗」的入口
 */
function updateLabel(state: UpdateState, pct: number): string {
    return {
        idle: t("settings.check-updates-2", "检查更新"),
        checking: t("settings.checking", "检查中…"),
        latest: t("settings.date", "已是最新版本"),
        available: t("settings.update-found", "发现新版本"),
        downloading: t("settings.update-downloading", "下载中 · {{pct}}%", { pct }),
        verifying: t("settings.update-verifying", "校验中…"),
        ready: t("settings.update-ready", "已验签 · 待安装"),
    }[state];
}

/**
 * 语言四档（「外观与关于」的第一行）。
 *
 * 三种语言的**自称按原样显示、不进翻译目录**——这是行业惯例：用户在自己的语言里才一眼认出
 * 「繁體中文」，翻成 "Traditional Chinese" 反而要他想一下是哪档。只有「跟随系统」是普通文案，
 * 所以这张表要在渲染时过一层 `t`（见 `localeOptions`）。
 */
const LOCALE_TABS: SelectOption[] = LOCALE_OPTIONS.map(({ value, label }) => ({
    value,
    label,
}));

const localeOptions = (t: TranslateFn): SelectOption[] =>
    LOCALE_TABS.map((o) => (o.value === "auto" ? { ...o, label: t("settings.system-default", "跟随系统") } : o));

/**
 * 端信息反查那一轮「问谁」的三档（互斥 ⇒ 一枚下拉，不再用「总闸 + 子开关」两颗控件管同一条链）。
 * `off` 就是原来那颗「联网反查」开关关上的效果；`minekuai` **只发麦块**、不悄悄回落官方
 * （见 Rust `env::index::resolve_online`：存活自查不过就如实报「未全部完成」）。
 * 表建在函数里、每档一条 `t(字面量)`，否则切语言不跟着换、`i18n check` 也扫不到。
 */
const envLookupOptions = (t: TranslateFn): SelectOption[] => [
    { value: "off", label: t("settings.lookup-off", "不反查") },
    // 「官方源」与上面「下载源」那档用的是同一个键：同一份中文只登记一次，译文也不会两处岔开
    { value: "official", label: t("backend.official-source", "官方源") },
    { value: "minekuai", label: t("settings.source-minekuai", "麦块 API") },
];

/** Modrinth 查询的两档（同一份数据、两条路线，所以也是下拉而不是开关）：
 *  mcimirror 是透明反向代理，官方是兜底那一条；CurseForge 的数据始终经 mcimirror，不看这一档 */
const modrinthQueryOptions = (t: TranslateFn): SelectOption[] => [
    { value: "mirror", label: t("settings.query-prefer-mirror", "优先镜像") },
    { value: "official", label: t("settings.query-only-official", "只用官方") },
];

/** 生效档 → 自称（「跟随系统」那行的说明要说出现在实际是哪一档） */
const endonymOf = (lng: Locale) =>
    LOCALE_OPTIONS.find((o) => o.value === LOCALE_TO_WIRE[lng])?.label ?? lng;

/** 三种清理：无用文件 / 只清过期缓存 / 清空全部缓存 */
type CleanKind = "junk" | "stale" | "all";

/** 播报文案的名词，与按钮上的说法一致，免得提示和界面两种叫法 */
function cleanLabel(kind: CleanKind): string {
    return {
        junk: t("settings.junk-files", "无用文件"),
        stale: t("settings.expired-cache", "过期缓存"),
        all: t("settings.download-cache", "下载缓存"),
    }[kind];
}

/** 灰掉也要能悬出原因：Btn/IconBtn 给禁用态关了 pointer-events，不放开就永远弹不出 Tip */
const STILL_HOVERABLE = "disabled:pointer-events-auto disabled:cursor-not-allowed";

/** 刷新图标转圈的最短时长，同时也是**一圈的周期**（下面那条 `animate-[spin_…]` 里同一个数）。
 *  要的是"看得见跑了一趟"，不是精确耗时：扫描常在半秒内回来，兜住这一档才不会读成"按了没反应"。
 *  周期没有沿用 Tailwind 的 `animate-spin`（1s 一圈）——600ms 的窗口只够转半圈，
 *  停在倒过来的姿势上，看着像图标歪了一下又弹回去。 */
const SPIN_MIN_MS = 600;
/** 周期写在字面量里，不是拼出来的：Tailwind 只扫源码字面，插值进类名的工具类不会生成 CSS。
 *  所以这个 600ms 必须与上面那个数手动同步。 */
const SPIN_CLASS = "[&>svg]:animate-[spin_600ms_linear_infinite]";

/** 无用文件的三项合计：一个孤儿暂存目录算一项，与后端报账口径一致 */
const junkCount = (u: CacheUsage) => u.partsCount + u.orphanCount + u.emptyDirs;
const junkBytes = (u: CacheUsage) => u.partsBytes + u.orphanBytes;

/** 设置里没选过时该收哪条更新线：与后端 check_update 用同一条判据（见 api.AUTO_UPDATE_CHANNEL） */
const channelOf = (s: AppSettings) => s.updateChannel ?? api.AUTO_UPDATE_CHANNEL;

/** 结果播报按真实回收量说话；删不动的要显出来，不能混在成功里 */
function cleanNotice(kind: CleanKind, r: CleanReport): { text: string; kind: NoticeKind } {
    const name = cleanLabel(kind);
    if (!r.items) {
        return r.failed
            ? { text: t("settings.use-another", "{{name}}正被其它程序占用，一项都没能删掉", { name }), kind: "warn" }
            : { text: t("settings.nothing-clean", "没有需要清理的{{name}}", { name }), kind: "info" };
    }
    const text = t("settings.name-count", "已清理{{name}} {{count}} 项 · {{size}}", {
        name,
        count: r.items,
        size: formatSize(r.bytes),
    });
    return r.failed
        ? { text: text + t("settings.count-still", "；另有 {{count}} 项占用中未删", { count: r.failed }), kind: "warn" }
        : { text, kind: "success" };
}

/** 下载源候选的上一份：这一档的列表整个进程不会变，重挂载再读一次必然同一份，
 *  但首帧空数组会让那一行的下拉先空一下再落回当前档 ⇒ 缓存留着当首帧初值 */
let cachedSources: SelectOption[] | null = null;

/**
 * 上一次占用扫描的结果，连同**它测的是哪个目录**（模块级，跨页面挂载存续）。
 *
 * 为什么不像设置/任务列表那样收进 `@/lib/api`：这份数字的有效性挂在目录上，
 * 而目录只有这一页在用；换一个目录，上一份立刻是假的，所以 key 必须跟着存。
 *
 * 为什么换页回来不必再扫：磁盘不会因为换页而变，真扫一次可能几千条目，换来换去必是同一份，
 * 重扫只换来一帧「占用统计中…」加刷新图标空转。要新数字有手动刷新那条路。
 */
let lastUsage: { dir: string; usage: CacheUsage } | null = null;

export function SettingsPage() {
    // 首帧吃上一份设置（`api.peekSettings`）：换页会把整页重挂载，没有它就得先画一屏骨架、
    // 再在整页淡入的半途中把每一行换成真内容——那一下读起来像两层不同数据的页面叠在一起
    const [settings, setSettings] = useState<AppSettings | null>(() => api.peekSettings());
    const [sources, setSources] = useState<SelectOption[]>(() => cachedSources ?? []);
    // 首帧吃上次扫描的结果（目录对得上才吃）：这样「占用统计中…」只在真的没扫过时出现一次
    const [usage, setUsage] = useState<CacheUsage | null>(() =>
        settings && lastUsage && lastUsage.dir === settings.cacheDir ? lastUsage.usage : null
    );
    /** 扫描占用进行中：只喂给刷新图标的转圈动效和它的禁用态 */
    const [refreshing, setRefreshing] = useState(false);
    const [cleaning, setCleaning] = useState<CleanKind | null>(null);
    /** 改到影响判定口径的全局开关时，作废转换页那份跨页草稿（见 pack-store） */
    const { clearDraft } = usePackStore();
    const t = useT();
    const [theme] = useTheme();
    /** 更新这一路的本地事实全在 store：弹窗挂在 Shell、角标在侧栏，这页只是第三个读它的人
     *  （自己记一份就会有两套账——换页回来那份就旧了） */
    const upd = useUpdate();
    /** 切到 Beta 的二次确认：改动的是"以后会装上什么包"，点一下就换太轻率 */
    const [confirmBeta, setConfirmBeta] = useState(false);

    useEffect(() => {
        void api
            .getSettings()
            .then(setSettings)
            .catch((e) => notify(t("settings.failed-read-settings", "读取设置失败：{{reason}}", { reason: errOf(e) }), "error"));
        void api
            .listDownloadSources()
            .then((list) => {
                const next = list.map(({ value, label, recommended }) => ({ value, label, recommended }));
                cachedSources = next;
                setSources(next);
            })
            .catch((e) => notify(t("settings.failed-read", "读取下载源失败：{{reason}}", { reason: errOf(e) }), "error"));
    }, []);

    /** 取件状态与进度事件的接线都在 `lib/update-store`（那边只接一次，这页读现成的） */

    /** 占用数字读的是磁盘：只在「这个进程还没扫过」、换缓存目录、手动刷新、清理完这四件事
     *之后各扫一遍，不轮询、也不跟换页重挂载走。
     * `refreshing` 是这一趟的在飞标记——刷新键是纯图标按钮，没有「统计中…」的文字可挂，
     * 只能靠它让图标转起来。收尾跟着 promise 走（自己掐表准不了真实耗时），
     * 但补一个最短时长：小缓存十几毫秒就回来，true→false 挤进同一次绘制等于一帧没画。 */
    const readUsage = useCallback(() => {
        const startedAt = performance.now();
        setRefreshing(true);
        void api
            .getCacheUsage()
            .then((u) => {
                setUsage(u);
                // 只在成功那一路记账：扫失败时留下的必须是上一份真数字（它测的还是同一个目录），
                // 而不是让下一次进页又白扫一遍
                const dir = api.peekSettings()?.cacheDir;
                if (dir) lastUsage = { dir, usage: u };
            })
            .catch((e) => notify(t("settings.failed-read-cache", "读取缓存占用失败：{{reason}}", { reason: errOf(e) }), "error"))
            .finally(() => {
                const left = SPIN_MIN_MS - (performance.now() - startedAt);
                if (left > 0) window.setTimeout(() => setRefreshing(false), left);
                else setRefreshing(false);
            });
    }, []);

    // 目录一改统计对象就换了，旧数字立刻是假的 ⇒ 跟着它重扫。
    // 这一句同时兼作冷启动那一趟（`lastUsage` 还是 null）；换页回来目录没变就直接跳过，
    // 首帧的数字由上面那份 `lastUsage` 初值给出，不会再转一圈空圈。
    useEffect(() => {
        if (settings && lastUsage?.dir !== settings.cacheDir) readUsage();
    }, [settings?.cacheDir, readUsage]);

    /** 局部更新 + 立即持久化。副作用不能写在 setState 的 updater 里（那个函数按契约是纯函数，
     * React 重跑一次就多写一次盘），所以先在事件里算好下一份，再落盘。
     * 返回是否真的写进去了：调用方要拿它决定"要不要宣布已生效"，不能自己以为成功。 */
    const patch = async (p: Partial<AppSettings>): Promise<boolean> => {
        if (!settings) return false;
        const next = { ...settings, ...p };
        setSettings(next);
        try {
            await api.saveSettings(next);
            // 这三项直接决定方案怎么判、联网那一轮还发不发 ⇒ 改过就得作废转换页草稿，
            // 下次进来重算。其余设置（输出目录/主题/更新渠道/Modrinth 镜像…）不参与判定，不该顺手抹掉手改
            if (
                p.stripClientOnly !== undefined ||
                p.envLookupSource !== undefined ||
                p.envLookupMcmod !== undefined
            ) {
                clearDraft();
            }
            return true;
        } catch (e) {
            notify(t("settings.couldn-save", "设置未能保存：{{reason}}", { reason: errOf(e) }), "error");
            // 后端拒绝写入时内存里还是旧值：重读一次，别让界面留着没生效的新值
            void api.getSettings().then(setSettings);
            return false;
        }
    };

    /** 选目录：取消返回 null，不改动 */
    const pickDir = async (key: "cacheDir" | "outputDir") => {
        try {
            const dir = await api.pickDirectory();
            if (!dir) return;
            await patch(key === "cacheDir" ? { cacheDir: dir } : { outputDir: dir });
        } catch (e) {
            notify(t("settings.couldn-open", "打开目录选择器失败：{{reason}}", { reason: errOf(e) }), "error");
        }
    };

    /**
     * 语言：**先落盘，再切当前进程**。
     *
     * 反过来的时候这里出过错（旧顺序：`applyLocaleChoice` → `patch`）：`applyLocaleChoice` 会让
     * `LocaleGate` 换 key 重挂整棵视图，设置页当场被拆掉，于是
     *  1. `patch` 里那次 `setSettings` 落在已卸载的实例上（React 直接丢弃），算好的新档位没人接；
     *  2. 新实例挂载时又要读一次设置，这一趟 `get_settings` 与上一趟 `set_settings` 抢同一个文件，
     *     读赢了就把改档**之前**的那份装回下拉框：界面已经是新语言，下拉却指着旧一档；
     *     再去点那一档撞上 `settings.locale === next` 提前返回，看着就是「选的语言不对、点了没反应」。
     *
     * 先写盘就没那个岔口：重挂之后首帧吃的 `peekSettings()` 与重读的那一趟都是新档。
     * 生效地确实在前端（翻译目录打进 bundle），但这一趟等的只是本机一次文件写；换来的是
     * 「下拉显示的选择 == 磁盘那份 == 当前语言」三方一致。写盘失败时 `patch` 自己弹「未能保存」
     * 并重读设置，语言压根没切过，不必再往回翻一次整个界面。
     */
    const chooseLocale = async (next: AppLocale) => {
        if (!settings || settings.locale === next) return;
        if (await patch({ locale: next })) await applyLocaleChoice(next);
    };

    /**
     * 跑一种清理。清完一定要重读占用：数字是磁盘的事实，删完还挂着旧数，
     * 用户会以为没删（或者以为删多了）。
     */
    const runClean = async (kind: CleanKind) => {
        if (cleaning) return;
        setCleaning(kind);
        try {
            const r = kind === "junk" ? await api.cleanJunk() : await api.cleanCache(kind);
            const n = cleanNotice(kind, r);
            notify(n.text, n.kind);
        } catch (e) {
            // 后端的拒绝是有原因的（比如转换进行中要清空全部），原话贴出来，别自己编
            notify(t("settings.failed-clean", "清理{{name}}失败：{{reason}}", { name: cleanLabel(kind), reason: errOf(e) }), "error");
        } finally {
            setCleaning(null);
            readUsage();
        }
    };

    /** 切渠道：往 Beta 走要过确认，往正式版走不需要拦（保守方向不出错） */
    const chooseChannel = (k: UpdateChannel) => {
        if (!settings || k === channelOf(settings)) return;
        if (k === "beta") setConfirmBeta(true);
        else void patch({ updateChannel: k });
    };

    const confirmBetaChannel = async () => {
        setConfirmBeta(false);
        if (await patch({ updateChannel: "beta" })) {
            notify(t("settings.switched-beta", "已切到 Beta：之后检查更新收到的会是测试版"), "warn");
        }
    };

    /** 这一行说什么由「那一轮」优先：它比一次网络结论更贴近用户此刻等的事。
     *  没轮次时才看「查」在不在飞、以及上一次查出来的结论——三样都读 store，这页不另记一份 */
    const round = roundOf(upd.status.stage);
    const updateState: UpdateState =
        round ??
        (upd.checking ? "checking" : upd.info ? (upd.info.hasUpdate ? "available" : "latest") : "idle");

    // 只有这个进程还没读过设置时才占位（冷启动那一趟；换页回来首帧就有 `peekSettings()`）：
    // 直接 return null 会让整页闪一下白，所以给一组等高占位行
    if (!settings) return <SettingsSkeleton />;

    return (
        <motion.div
            variants={PAGE_RISE}
            initial="hidden"
            animate="show"
            className="flex flex-col gap-5 py-6"
        >
            <PageHeader compact title={t("common.settings", "设置")} sub={t("settings.conversion-storage", "转换行为、存储、网络与外观偏好")} />

            <div className="flex w-full flex-col gap-5">
                {/* ---- 转换选项 ---- */}
                <Section title={t("settings.conversion-options", "转换选项")}>
                    <SettingRow label={t("settings.server-output", "服务端输出目录")} desc={t("settings.where-finished", "转换完成的整合包落位目录")}>
                        <TextInput
                            plain
                            icon={Folder}
                            readOnly
                            value={settings.outputDir}
                            onClick={() => void pickDir("outputDir")}
                            className="w-[300px] cursor-pointer"
                        />
                        <Btn size="sm" onClick={() => void pickDir("outputDir")}>
                            {t("settings.choose", "选择")}
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label={t("settings.strip-client", "剔除客户端专属资源")}
                        desc={t("settings.auto-removes", "按端证据自动移除光影、资源包等客户端资源")}
                    >
                        <Toggle
                            size="md"
                            checked={settings.stripClientOnly}
                            onChange={(v) => void patch({ stripClientOnly: v })}
                        />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.verify-after", "构建后自检")}
                        desc={t("settings.post-build", "打包完成后离线对账产物：模组是否齐、jar 是否完整、依赖是否被误剔")}
                    >
                        <Toggle
                            size="md"
                            checked={settings.verifyAfterBuild}
                            onChange={(v) => void patch({ verifyAfterBuild: v })}
                        />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.install-loader", "本机安装 Loader")}
                        desc={t("settings.runs-forge", "Forge / NeoForge 转换时在本机跑 installer，产物上传即可开服（需本机 Java）")}
                    >
                        <Toggle
                            size="md"
                            checked={settings.installLoaderLocally}
                            onChange={(v) => void patch({ installLoaderLocally: v })}
                        />
                    </SettingRow>
                    {/* 关着「本机安装」时这一行没有对象可复用，留着就是个恒为 0 的噪音（同自检的 keep 项口径）。
                        进出走 Collapse：Section 的 Panel 是 gap=0 的 divide-y 卡 ⇒ gap 传 0；
                        折叠壳不带线也不带内边距（壳带 1px 就收不到 0），线由上一行的 divide 边画，
                        壳是末子元素时 `.divide-y > :not(:last-child)` 不会给它加边框 */}
                    <Collapse when={settings.installLoaderLocally} gap={0}>
                        <SettingRow
                            label={t("settings.reuse-installed", "复用已装的 Loader")}
                            desc={t("settings.cache-loader", "按加载器与版本存进缓存，同版本的第二包起不再重下重装")}
                        >
                            <Toggle
                                size="md"
                                checked={settings.reuseLoaderInstalls}
                                onChange={(v) => void patch({ reuseLoaderInstalls: v })}
                            />
                        </SettingRow>
                    </Collapse>
                </Section>

                {/* ---- 存储与缓存 ---- */}
                <Section title={t("settings.storage-cache", "存储与缓存")}>
                    <SettingRow label={t("settings.cache-folder", "工作缓存目录")} desc={t("settings.download-cache-extraction", "下载缓存、解包与构建中间产物")}>
                        <TextInput
                            plain
                            icon={Folder}
                            readOnly
                            value={settings.cacheDir}
                            onClick={() => void pickDir("cacheDir")}
                            className="w-[300px] cursor-pointer"
                        />
                        <Btn size="sm" onClick={() => void pickDir("cacheDir")}>
                            {t("settings.choose", "选择")}
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label={t("settings.download-cache", "下载缓存")}
                        desc={
                            !usage
                                ? t("settings.counting-usage", "占用统计中…")
                                : usage.filesCount === 0
                                  ? t("settings.mod-files", "还没有下载过模组文件")
                                  : t("settings.count-file", "{{count}} 个文件 · {{size}}", {
                                        count: usage.filesCount,
                                        size: formatSize(usage.filesBytes),
                                    }) +
                                    (usage.staleCount > 0
                                        ? t("settings.count-file-unused", "｜{{days}} 天未再使用 {{count}} 个 · {{size}}", {
                                              days: usage.staleDays,
                                              count: usage.staleCount,
                                              size: formatSize(usage.staleBytes),
                                          })
                                        : t("settings.used-days", "｜{{days}} 天内都用过，没有过期项", { days: usage.staleDays })) +
                                    // 灰掉的按钮悬不出气泡（pointer-events 关了），所以这句话得写在明面上
                                    (usage.busy ? t("settings.converting-clear", "｜转换进行中，暂不能清空全部") : "")
                        }
                    >
                        <IconBtn
                            icon={RefreshCw}
                            title={t("settings.recount-usage", "重新统计占用")}
                            /* 转圈只给图标本体（`[&>svg]`），不给按钮：整块转会把悬停底色和
                             * 按压缩放一起带歪。禁用态照 `cleaning` 那一档的规矩灰下来，
                             * 顺带挡住连点——两趟扫描并发回话会是后发先至的假数字。 */
                            className={cn(STILL_HOVERABLE, refreshing && SPIN_CLASS)}
                            disabled={!!cleaning || refreshing}
                            onClick={() => readUsage()}
                        />
                        <Btn
                            size="sm"
                            disabled={!usage || usage.staleCount === 0 || !!cleaning}
                            onClick={() => void runClean("stale")}
                        >
                            {cleaning === "stale" ? t("settings.cleaning", "清理中…") : t("settings.clean-expired", "清过期")}
                        </Btn>
                        <Btn
                            size="sm"
                            variant="danger"
                            disabled={
                                !usage || usage.filesCount === 0 || !!cleaning || usage.busy
                            }
                            onClick={() => void runClean("all")}
                        >
                            {cleaning === "all" ? t("settings.cleaning", "清理中…") : t("settings.clear", "清空全部")}
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label={t("settings.junk-files", "无用文件")}
                        desc={
                            !usage
                                ? t("settings.counting-usage", "占用统计中…")
                                : junkCount(usage) === 0
                                  ? t("settings.leftover-staging", "下载错误、崩溃留下的暂存目录、空壳目录")
                                  : `${junkBytes(usage) ? `${formatSize(junkBytes(usage))}：` : ""}` +
                                    t("settings.parts-partial", "半截下载 {{parts}} 个 · 残留暂存 {{orphan}} 个 · 空壳目录 {{empty}} 个", {
                                        parts: usage.partsCount,
                                        orphan: usage.orphanCount,
                                        empty: usage.emptyDirs,
                                    }) +
                                    (usage.busy ? t("settings.converting-partial", "｜转换进行中，正在写的半截下载不计入") : "")
                        }
                    >
                        <Btn
                            size="sm"
                            icon={Trash2}
                            disabled={!usage || !!cleaning}
                            onClick={() => void runClean("junk")}
                        >
                            {cleaning === "junk" ? t("settings.cleaning", "清理中…") : t("settings.clean", "清理")}
                        </Btn>
                    </SettingRow>
                </Section>

                {/* ---- 下载与查询：拉文件与问平台接口走哪条路线，全是「选一条路」的档位 ---- */}
                <Section title={t("settings.download-query", "下载与查询")}>
                    <SettingRow label={t("settings.download-source", "下载源")} desc={t("settings.prefer-cn", "版本表与加载器 jar 优先走国内镜像，不通自动回落官方")}>
                        <SearchSelect
                            plain
                            value={settings.downloadSource}
                            options={sources}
                            onChange={(v) => void patch({ downloadSource: v as AppSettings["downloadSource"] })}
                            className="w-[196px]"
                        />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.modrinth-query", "Modrinth 查询")}
                        desc={t(
                            "settings.modrinth-mirror-desc",
                            "网络添加的搜索/详情/版本/译文优先走 mcimirror 镜像、官方自动兜底；CurseForge 数据始终经 mcimirror 获取（无需 API Key）"
                        )}
                    >
                        <SearchSelect
                            plain
                            value={settings.modrinthMirror ? "mirror" : "official"}
                            options={modrinthQueryOptions(t)}
                            onChange={(v) => void patch({ modrinthMirror: v === "mirror" })}
                            className="w-[196px]"
                        />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.parallel-downloads", "并发下载数")}
                        desc={t("settings.threads-mod", "同时拉取模组与服务端文件的线程数（1–16）；端信息反查用的是固定的并发")}
                    >
                        <Stepper
                            plain
                            min={1}
                            max={16}
                            value={settings.concurrency}
                            onChange={(v) => void patch({ concurrency: v })}
                        />
                    </SettingRow>
                </Section>

                {/* ---- 端信息反查：一条链的两个档位——问谁，以及它答不上时补不补 ---- */}
                <Section title={t("settings.env-lookup", "端信息反查")}>
                    <SettingRow
                        label={t("settings.lookup-source", "反查源")}
                        desc={
                            settings.envLookupSource === "off"
                                ? t("settings.lookup-off-desc", "不发联网请求，只用包内自证、本地索引与名称兜底")
                                : t("settings.env-lookup-source-desc", "包内证据不足时联网反查该构建的端支持度并本地缓存；选一个源，不会两个都问")
                        }
                    >
                        <SearchSelect
                            plain
                            value={settings.envLookupSource}
                            options={envLookupOptions(t)}
                            onChange={(v) => void patch({ envLookupSource: v as AppSettings["envLookupSource"] })}
                            className="w-[196px]"
                        />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.mcmod-lookup", "百科补全")}
                        desc={
                            // 关掉反查时这一档压根没有生效对象：灰掉 + 把理由写在同一行，不再另起气泡
                            settings.envLookupSource === "off"
                                ? t("settings.mcmod-lookup-inert", "反查关掉时这一档不起作用")
                                : t(
                                      "settings.mcmod-lookup-desc",
                                      "平台各腿全答不上时查 MC百科词条的「运行环境」（非官方接口，默认关）"
                                  )
                        }
                    >
                        <Toggle
                            size="md"
                            disabled={settings.envLookupSource === "off"}
                            checked={settings.envLookupMcmod}
                            onChange={(v) => void patch({ envLookupMcmod: v })}
                        />
                    </SettingRow>
                </Section>

                {/* ---- 外观与关于 ---- */}
                <Section title={t("settings.appearance-about", "外观与关于")}>
                    <SettingRow
                        label={t("settings.language", "界面语言")}
                        desc={
                            settings.locale === "auto"
                                ? t("settings.follows-system", "跟随系统（当前：{{name}}）", { name: endonymOf(systemLocale()) })
                                : t("settings.pinned-choice-longer", "已按你的选择固定，不再随系统语言变化")
                        }
                    >
                        {/* 四档一行下拉，不用 SegTabs：English/繁體中文 这类自称当分段标签会把行撑得比别处宽 */}
                        <SearchSelect
                            plain
                            value={settings.locale}
                            options={localeOptions(t)}
                            onChange={(v) => void chooseLocale(v as AppLocale)}
                            className="w-[184px]"
                        />
                    </SettingRow>
                    <SettingRow label={t("settings.theme", "主题")} desc={t("settings.defaults-system", "默认跟随系统，也可锁定浅色 / 深色")}>
                        {/* 圆心取被点的那一格：胶囊滑到位之后颜色才从它铺开，两处入口时序一致 */}
                        <SegTabs items={themeTabs()} value={theme} onChange={(t, el) => switchTheme(t, el)} />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.update-channel", "更新渠道")}
                        desc={
                            settings.updateChannel
                                ? t("settings.pinned-choice", "已按你的选择固定，不再看本机版本号判档")
                                : t("settings.pinned-follows", "当前构建版本（{{channel}}）", {
                                      channel: channelLabel(channelOf(settings)),
                                  })
                        }
                    >
                        <SegTabs
                            items={channelTabs()}
                            value={channelOf(settings)}
                            onChange={chooseChannel}
                        />
                    </SettingRow>
                    <SettingRow
                        label={t("settings.version", "版本")}
                        descMono
                        desc={`v${api.APP_VERSION}`}
                    >
                        <Btn
                            size="sm"
                            icon={RefreshCw}
                            onClick={() => {
                                // 那一轮在跑或已就位时这颗钮是「回到它」：重查会把用户正看着的进度顶掉。
                                // 结论现在住在 store 里、跨页留着，所以「查过且有新版」同样直接回那扇窗
                                if (upd.info && (round || upd.info.hasUpdate)) openUpdate();
                                else void runCheck();
                            }}
                        >
                            {updateLabel(updateState, ratioPercent(upd.status.downloaded, upd.status.total))}
                        </Btn>
                    </SettingRow>
                </Section>
            </div>

            {/* 切 Beta 的二次确认：动的以后是"装上哪个包"，所以照全应用规矩只走按钮关闭 */}
            <ModalShell
                open={confirmBeta}
                onClose={() => setConfirmBeta(false)}
                persistent
                width={420}
                title={t("settings.switch-beta", "切到测试版（Beta）渠道")}
                sub={t("settings.check-updates", "之后「检查更新」找到的是还没定型的测试包")}
                icon={FlaskConical}
                footerNote={t("settings.switch-back", "随时能切回正式版；测试包可能带着没测出来的问题")}
                footerActions={
                    <>
                        <Btn size="sm" onClick={() => setConfirmBeta(false)}>
                            {t("common.cancel", "取消")}
                        </Btn>
                        <Btn size="sm" variant="primary" onClick={() => void confirmBetaChannel()}>
                            {t("settings.confirm-switch", "确认切换")}
                        </Btn>
                    </>
                }
            >
                <p className="text-[12px] leading-[20px] text-text-2">
                    {t(
                        "settings.beta-gets", "测试版先拿到新功能，也先拿到新问题：转换结果、任务存档都可能出没见过的状况。建议只在愿意顺手报 bug 的时候切过来。"
                    )}
                </p>
            </ModalShell>
        </motion.div>
    );
}

/**
 * 分组：等宽小标题 + 无内边距卡片（行间 1px $stroke-soft 分隔）。
 * 本身是页内错峰的一块（CARD_RISE），节奏由页面根容器给
 */
function Section({ title, children }: { title: string; children: ReactNode }) {
    return (
        <motion.section variants={CARD_RISE} className="flex w-full flex-col gap-2">
            <SectionTitle>{title}</SectionTitle>
            <Panel gap={0} className="divide-y divide-stroke-soft p-0">
                {children}
            </Panel>
        </motion.section>
    );
}

/** 读设置的往返期间占位：按真实分组与行数排，免得整页先白一下再蹦出内容 */
function SettingsSkeleton() {
    const t = useT();
    const groups: Array<[string, number]> = [
        [t("settings.conversion-options", "转换选项"), 5],
        [t("settings.storage-cache", "存储与缓存"), 3],
        [t("settings.download-query", "下载与查询"), 3],
        [t("settings.env-lookup", "端信息反查"), 2],
        [t("settings.appearance-about", "外观与关于"), 4],
    ];
    return (
        <div className="flex flex-col gap-5 py-6">
            <PageHeader compact title={t("common.settings", "设置")} sub={t("settings.conversion-storage", "转换行为、存储、网络与外观偏好")} />
            <div className="flex w-full flex-col gap-5">
                {groups.map(([title, rows]) => (
                    <Section key={title} title={title}>
                        {Array.from({ length: rows }, (_, i) => (
                            <div key={i} className="flex items-center justify-between gap-4 px-5 py-[14px]">
                                <div className="flex min-w-0 flex-col gap-1.5">
                                    <span className="h-[13px] w-24 animate-pulse rounded bg-stroke" />
                                    <span className="h-[10px] w-44 animate-pulse rounded bg-stroke-soft" />
                                </div>
                                <span className="h-8 w-40 shrink-0 animate-pulse rounded-lg bg-stroke" />
                            </div>
                        ))}
                    </Section>
                ))}
            </div>
        </div>
    );
}

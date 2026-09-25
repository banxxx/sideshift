/**
 * 设置页 Settings（SS.pen `F4na6`）
 *
 * 四个分组，每组 = 等宽小标题（11/600，字距 1.2）+ 一张无内边距卡片，
 * 行与行之间用 1px $stroke-soft 分隔（divide-y），行标尺 padding[14,20]。
 *  - 转换选项：服务端输出目录 / 剔除客户端专属资源 / 构建后自检 / 本机安装 Loader（含复用已装）
 *  - 存储与缓存：工作缓存目录 / 下载缓存 / 无用文件（占用数字来自后端真实扫描，一个目录一个进程只扫一次）
 *  - 网络：下载源 / CurseForge API Key / 联网反查端信息 / 并发下载数
 *  - 外观与关于：主题（三态分段）/ 更新渠道（正式版·Beta 两档，切 Beta 要确认）/ 版本（检查更新）
 * 读写走 @/lib/api 门面；主题走 @/lib/theme 单一真源（侧栏按钮同步）。
 * 设置是「改一处即持久化」，所以写盘失败必须外显（否则界面显示已生效、重启又回退），
 * 失败后从后端重读一次，让界面与真正常量的那份一致。
 */
import { ExternalLink, FlaskConical, Folder, Monitor, Moon, RefreshCw, Sun, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import * as api from "@/lib/api";
import { formatSize } from "@/lib/format";
import { notify, type NoticeKind } from "@/lib/notify";
import { CARD_RISE, PAGE_RISE } from "@/lib/page-motion";
import { usePackStore } from "@/lib/pack-store";
import { switchTheme, useTheme, type Theme } from "@/lib/theme";
import type { AppSettings, CacheUsage, CleanReport, UpdateChannel } from "@/lib/types";
import { cn } from "@/lib/utils";
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

const THEME_TABS: Array<{ key: Theme; label: string; icon: typeof Sun }> = [
    { key: "light", label: "浅色", icon: Sun },
    { key: "dark", label: "深色", icon: Moon },
    { key: "system", label: "系统", icon: Monitor },
];

type UpdateState = "idle" | "checking" | "latest" | "available";

/** 更新渠道两档：SegTabs 的 value 取解析后的档位，所以「跟随当前构建」在界面上不占第三态 */
const CHANNEL_TABS: Array<{ key: UpdateChannel; label: string }> = [
    { key: "stable", label: "正式版" },
    { key: "beta", label: "Beta" },
];

const UPDATE_LABEL: Record<UpdateState, string> = {
    idle: "检查更新",
    checking: "检查中…",
    latest: "已是最新版本",
    available: "发现新版本",
};

/** invoke reject 回来的可能是 Error 也可能是 Rust 的字符串消息 */
const errOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** 三种清理：无用文件 / 只清过期缓存 / 清空全部缓存 */
type CleanKind = "junk" | "stale" | "all";

/** 播报文案的名词，与按钮上的说法一致，免得提示和界面两种叫法 */
const CLEAN_LABEL: Record<CleanKind, string> = {
    junk: "无用文件",
    stale: "过期缓存",
    all: "下载缓存",
};

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

/** 渠道的中文说法，行说明与确认弹窗共用，免得一处写「Beta」一处写「测试版」 */
const channelLabel = (c: UpdateChannel) => (c === "beta" ? "测试版（Beta）" : "正式版");

/** 结果播报按真实回收量说话；删不动的要显出来，不能混在成功里 */
function cleanNotice(kind: CleanKind, r: CleanReport): { text: string; kind: NoticeKind } {
    const name = CLEAN_LABEL[kind];
    if (!r.items) {
        return r.failed
            ? { text: `${name}正被其它程序占用，一项都没能删掉`, kind: "warn" }
            : { text: `没有需要清理的${name}`, kind: "info" };
    }
    const text = `已清理${name} ${r.items} 项 · ${formatSize(r.bytes)}`;
    return r.failed
        ? { text: `${text}；另有 ${r.failed} 项占用中未删`, kind: "warn" }
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
    const [theme] = useTheme();
    const [update, setUpdate] = useState<UpdateState>("idle");
    /** 切到 Beta 的二次确认：改动的是"以后会装上什么包"，点一下就换太轻率 */
    const [confirmBeta, setConfirmBeta] = useState(false);
    /**
     * CurseForge Key 的本地草稿：设置是「改一处即持久化」的，但 Key 是手打的字符串，
     * 逐键写盘会把用户的输入中途存成半截。失焦（或回车）才落一次盘。
     */
    const [cfKey, setCfKey] = useState("");

    useEffect(() => {
        void api
            .getSettings()
            .then(setSettings)
            .catch((e) => notify(`读取设置失败：${errOf(e)}`, "error"));
        void api
            .listDownloadSources()
            .then((list) => {
                const next = list.map(({ value, label, recommended }) => ({ value, label, recommended }));
                cachedSources = next;
                setSources(next);
            })
            .catch((e) => notify(`读取下载源失败：${errOf(e)}`, "error"));
    }, []);

    /** 占用数字读的是磁盘：只在「这个进程还没扫过」、换缓存目录、手动刷新、清理完这四件事
     * 之后各扫一遍，不轮询、也不跟换页重挂载走。
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
            .catch((e) => notify(`读取缓存占用失败：${errOf(e)}`, "error"))
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
            // 这两项直接决定方案怎么判、联网那一轮还算不算在跑 ⇒ 改过就得作废转换页草稿，
            // 下次进来重算。其余设置（输出目录/主题/更新渠道…）不参与判定，不该顺手抹掉手改
            if (p.stripClientOnly !== undefined || p.autoClassifyOnline !== undefined) {
                clearDraft();
            }
            return true;
        } catch (e) {
            notify(`设置未能保存：${errOf(e)}`, "error");
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
            notify(`打开目录选择器失败：${errOf(e)}`, "error");
        }
    };

    // Key 的显示值跟着后端那一份走：保存失败时 patch 会重读设置，草稿也要跟着回位，
    // 否则输入框留着没生效的半截串，界面与常量又不一致了
    useEffect(() => {
        setCfKey(settings?.curseforgeApiKey ?? "");
    }, [settings?.curseforgeApiKey]);

    /** 失焦/回车时落盘：与已存的那份一样就什么都不做（不写盘、也不报「已保存」） */
    const commitCfKey = async () => {
        const v = cfKey.trim();
        if (v === (settings?.curseforgeApiKey ?? "").trim()) return;
        if (await patch({ curseforgeApiKey: v || null })) {
            notify(v ? "已保存 CurseForge API Key" : "已清除 CurseForge API Key", "success");
        }
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
            notify(`清理${CLEAN_LABEL[kind]}失败：${errOf(e)}`, "error");
        } finally {
            setCleaning(null);
            readUsage();
        }
    };

    /**
     * 检查更新。结论（有没有更新）由后端 semver 比出来，这里只负责播报，
     * 播报必带完整版本号：Beta 用户要看得见收到的是 1.1.0-beta.2 还是 1.1.0。
     */
    const checkUpdate = async () => {
        setUpdate("checking");
        try {
            const r = await api.checkUpdate();
            setUpdate(r.hasUpdate ? "available" : "latest");
            notify(
                r.hasUpdate
                    ? `发现新版本 v${r.latest}（当前 v${r.current}）`
                    : `已是最新版本 v${r.current}`,
                r.hasUpdate ? "info" : "success"
            );
        } catch (e) {
            // 网络不通、仓库还没发过 release 都会走到这里：只报错，不许顶着一个假的"已是最新"
            notify(`检查更新失败：${errOf(e)}`, "error");
            setUpdate("idle");
            return;
        }
        window.setTimeout(() => setUpdate("idle"), 3000);
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
            notify("已切到 Beta：之后检查更新收到的会是测试版", "warn");
        }
    };

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
            <PageHeader compact title="设置" sub="转换行为、存储、网络与外观偏好" />

            <div className="flex w-full flex-col gap-5">
                {/* ---- 转换选项 ---- */}
                <Section title="转换选项">
                    <SettingRow label="服务端输出目录" desc="转换完成的整合包落位目录">
                        <TextInput
                            plain
                            icon={Folder}
                            readOnly
                            value={settings.outputDir}
                            onClick={() => void pickDir("outputDir")}
                            className="w-[300px] cursor-pointer"
                        />
                        <Btn size="sm" onClick={() => void pickDir("outputDir")}>
                            选择
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label="剔除客户端专属资源"
                        desc="按端证据自动移除光影、小地图等客户端模组"
                    >
                        <Toggle
                            size="md"
                            checked={settings.stripClientOnly}
                            onChange={(v) => void patch({ stripClientOnly: v })}
                        />
                    </SettingRow>
                    <SettingRow
                        label="构建后自检"
                        desc="打包完成后离线对账产物：模组是否齐、jar 是否完整、依赖是否被误剔"
                    >
                        <Toggle
                            size="md"
                            checked={settings.verifyAfterBuild}
                            onChange={(v) => void patch({ verifyAfterBuild: v })}
                        />
                    </SettingRow>
                    <SettingRow
                        label="本机安装 Loader"
                        desc="Forge / NeoForge 转换时在本机跑 installer，产物上传即可开服（需本机 Java）"
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
                            label="复用已装的 Loader"
                            desc="按加载器与版本存进缓存，同版本的第二包起不再重下重装"
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
                <Section title="存储与缓存">
                    <SettingRow label="工作缓存目录" desc="下载缓存、解包与构建中间产物">
                        <TextInput
                            plain
                            icon={Folder}
                            readOnly
                            value={settings.cacheDir}
                            onClick={() => void pickDir("cacheDir")}
                            className="w-[300px] cursor-pointer"
                        />
                        <Btn size="sm" onClick={() => void pickDir("cacheDir")}>
                            选择
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label="下载缓存"
                        desc={
                            !usage
                                ? "占用统计中…"
                                : usage.filesCount === 0
                                  ? "还没有下载过模组文件"
                                  : `${usage.filesCount} 个文件 · ${formatSize(usage.filesBytes)}` +
                                    (usage.staleCount > 0
                                        ? `｜${usage.staleDays} 天未再使用 ${usage.staleCount} 个 · ${formatSize(usage.staleBytes)}`
                                        : `｜${usage.staleDays} 天内都用过，没有过期项`) +
                                    // 灰掉的按钮悬不出气泡（pointer-events 关了），所以这句话得写在明面上
                                    (usage.busy ? "｜转换进行中，暂不能清空全部" : "")
                        }
                    >
                        <IconBtn
                            icon={RefreshCw}
                            title="重新统计占用"
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
                            {cleaning === "stale" ? "清理中…" : "清过期"}
                        </Btn>
                        <Btn
                            size="sm"
                            variant="danger"
                            disabled={
                                !usage || usage.filesCount === 0 || !!cleaning || usage.busy
                            }
                            onClick={() => void runClean("all")}
                        >
                            {cleaning === "all" ? "清理中…" : "清空全部"}
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label="无用文件"
                        desc={
                            !usage
                                ? "占用统计中…"
                                : junkCount(usage) === 0
                                  ? "下载错误、崩溃留下的暂存目录、空壳目录"
                                  : `${junkBytes(usage) ? `${formatSize(junkBytes(usage))}：` : ""}半截下载 ${usage.partsCount} 个 · 残留暂存 ${usage.orphanCount} 个 · 空壳目录 ${usage.emptyDirs} 个` +
                                    (usage.busy ? "｜转换进行中，正在写的半截下载不计入" : "")
                        }
                    >
                        <Btn
                            size="sm"
                            icon={Trash2}
                            disabled={!usage || !!cleaning}
                            onClick={() => void runClean("junk")}
                        >
                            {cleaning === "junk" ? "清理中…" : "清理"}
                        </Btn>
                    </SettingRow>
                </Section>

                {/* ---- 网络 ---- */}
                <Section title="网络">
                    <SettingRow label="下载源" desc="版本表与加载器 jar 优先走国内镜像，不通自动回落官方">
                        <SearchSelect
                            plain
                            value={settings.downloadSource}
                            options={sources}
                            onChange={(v) => void patch({ downloadSource: v as AppSettings["downloadSource"] })}
                            className="w-[160px]"
                        />
                    </SettingRow>
                    <SettingRow
                        label="CurseForge API Key"
                        desc={
                            settings.curseforgeApiKey
                                ? "已配置：网络添加里的 CurseForge 搜索与构建列表可用"
                                : "未配置：CurseForge API Key，点「获取」填表申请"
                        }
                    >
                        <TextInput
                            plain
                            value={cfKey}
                            placeholder="粘贴 API Key"
                            spellCheck={false}
                            autoComplete="off"
                            className="w-[220px]"
                            onChange={(e) => setCfKey(e.target.value)}
                            onBlur={() => void commitCfKey()}
                            onKeyDown={(e) => {
                                // 回车算「输完了」：借用失焦走同一条落盘路径，不用另加一个保存按钮
                                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                            }}
                        />
                        <Btn
                            size="sm"
                            icon={ExternalLink}
                            onClick={() => void api.openExternal(api.CURSEFORGE_APPLY_FORM)}
                        >
                            获取
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label="联网反查端信息"
                        desc="包内证据不足时，按 sha1 向 Modrinth 查该构建的端支持度并本地缓存"
                    >
                        <Toggle
                            size="md"
                            checked={settings.autoClassifyOnline}
                            onChange={(v) => void patch({ autoClassifyOnline: v })}
                        />
                    </SettingRow>
                    <SettingRow
                        label="并发下载数"
                        desc="同时拉取模组 jar 与反查端信息的线程数（1–16）"
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

                {/* ---- 外观与关于 ---- */}
                <Section title="外观与关于">
                    <SettingRow label="主题" desc="默认跟随系统，也可锁定浅色 / 深色">
                        {/* 圆心取被点的那一格：胶囊滑到位之后颜色才从它铺开，两处入口时序一致 */}
                        <SegTabs items={THEME_TABS} value={theme} onChange={(t, el) => switchTheme(t, el)} />
                    </SettingRow>
                    <SettingRow
                        label="更新渠道"
                        desc={
                            settings.updateChannel
                                ? "已按你的选择固定，不再看本机版本号判档"
                                : `未固定：跟随当前构建（现在收${channelLabel(channelOf(settings))}）`
                        }
                    >
                        <SegTabs
                            items={CHANNEL_TABS}
                            value={channelOf(settings)}
                            onChange={chooseChannel}
                        />
                    </SettingRow>
                    <SettingRow
                        label="版本"
                        descMono
                        desc={`v${api.APP_VERSION}`}
                    >
                        <Btn size="sm" icon={RefreshCw} onClick={() => void checkUpdate()}>
                            {UPDATE_LABEL[update]}
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
                title="切到测试版（Beta）渠道"
                sub="之后「检查更新」找到的是还没定型的测试包"
                icon={FlaskConical}
                footerNote="随时能切回正式版；测试包可能带着没测出来的问题"
                footerActions={
                    <>
                        <Btn size="sm" onClick={() => setConfirmBeta(false)}>
                            取消
                        </Btn>
                        <Btn size="sm" variant="primary" onClick={() => void confirmBetaChannel()}>
                            确认切换
                        </Btn>
                    </>
                }
            >
                <p className="text-[12px] leading-[20px] text-text-2">
                    测试版先拿到新功能，也先拿到新问题：转换结果、任务存档都可能出没见过的状况。
                    建议只在愿意顺手报 bug 的时候切过来。
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
    const groups: Array<[string, number]> = [
        ["转换选项", 2],
        ["存储与缓存", 3],
        ["网络", 5],
        ["外观与关于", 3],
    ];
    return (
        <div className="flex flex-col gap-5 py-6">
            <PageHeader compact title="设置" sub="转换行为、存储、网络与外观偏好" />
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

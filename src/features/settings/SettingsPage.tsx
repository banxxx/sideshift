/**
 * 设置页 Settings（SS.pen `F4na6`）
 *
 * 四个分组，每组 = 等宽小标题（11/600，字距 1.2）+ 一张无内边距卡片，
 * 行与行之间用 1px $stroke-soft 分隔（divide-y），行标尺 padding[14,20]。
 *  - 转换选项：服务端输出目录 / 剔除客户端专属资源
 *  - 存储与缓存：工作缓存目录 / 下载缓存 / 无用文件（占用数字来自后端真实扫描）
 *  - 网络：下载源 / 构建后自检 / 联网反查端信息 / 并发下载数
 *  - 外观与关于：主题（三态分段）/ 版本（仓库外链 + 检查更新）
 * 读写走 @/lib/api 门面；主题走 @/lib/theme 单一真源（侧栏按钮同步）。
 * 设置是「改一处即持久化」，所以写盘失败必须外显（否则界面显示已生效、重启又回退），
 * 失败后从后端重读一次，让界面与真正常量的那份一致。
 */
import { ExternalLink, Folder, Monitor, Moon, RefreshCw, Sun, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import * as api from "@/lib/api";
import { formatSize } from "@/lib/format";
import { notify, type NoticeKind } from "@/lib/notify";
import { switchTheme, useTheme, type Theme } from "@/lib/theme";
import type { AppSettings, CacheUsage, CleanReport } from "@/lib/types";
import {
    Btn,
    IconBtn,
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

/** 无用文件的三项合计：一个孤儿暂存目录算一项，与后端报账口径一致 */
const junkCount = (u: CacheUsage) => u.partsCount + u.orphanCount + u.emptyDirs;
const junkBytes = (u: CacheUsage) => u.partsBytes + u.orphanBytes;

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

export function SettingsPage() {
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [sources, setSources] = useState<SelectOption[]>([]);
    const [usage, setUsage] = useState<CacheUsage | null>(null);
    const [cleaning, setCleaning] = useState<CleanKind | null>(null);
    const [theme] = useTheme();
    const [update, setUpdate] = useState<UpdateState>("idle");

    useEffect(() => {
        void api
            .getSettings()
            .then(setSettings)
            .catch((e) => notify(`读取设置失败：${errOf(e)}`, "error"));
        void api
            .listDownloadSources()
            .then((list) => setSources(list.map(({ value, label, recommended }) => ({ value, label, recommended }))))
            .catch((e) => notify(`读取下载源失败：${errOf(e)}`, "error"));
    }, []);

    /** 占用数字读的是磁盘：只在进页、换缓存目录、清理完这三件事之后各扫一遍，不轮询 */
    const readUsage = useCallback(() => {
        void api
            .getCacheUsage()
            .then(setUsage)
            .catch((e) => notify(`读取缓存占用失败：${errOf(e)}`, "error"));
    }, []);

    // 等设置到手再扫：cacheDir 一改统计对象就换了个目录，旧数字立刻是假的，所以跟着它重读
    useEffect(() => {
        if (settings) readUsage();
    }, [settings?.cacheDir, readUsage]);

    /** 局部更新 + 立即持久化。副作用不能写在 setState 的 updater 里（那个函数按契约是纯函数，
     * React 重跑一次就多写一次盘），所以先在事件里算好下一份，再落盘。 */
    const patch = async (p: Partial<AppSettings>) => {
        if (!settings) return;
        const next = { ...settings, ...p };
        setSettings(next);
        try {
            await api.saveSettings(next);
        } catch (e) {
            notify(`设置未能保存：${errOf(e)}`, "error");
            // 后端拒绝写入时内存里还是旧值：重读一次，别让界面留着没生效的新值
            void api.getSettings().then(setSettings);
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

    const checkUpdate = async () => {
        setUpdate("checking");
        const latest = await api.checkUpdate();
        setUpdate(latest === api.APP_VERSION ? "latest" : "available");
        window.setTimeout(() => setUpdate("idle"), 3000);
    };

    // 后端读设置是异步的：直接 return null 会让整页闪一下白，给一组等高占位行
    if (!settings) return <SettingsSkeleton />;

    return (
        <div className="flex flex-col gap-5">
            <PageHeader compact title="设置" sub="转换行为、存储、网络与外观偏好" />

            <div className="flex w-full flex-col gap-5">
                {/* ---- 转换选项 ---- */}
                <Section title="转换选项">
                    <SettingRow label="服务端输出目录" desc="转换完成的整合包落位到这里（单个包可在转换页覆写）">
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
                        desc="按端证据自动移除光影、小地图等客户端模组；关则全部保留"
                    >
                        <Toggle
                            size="md"
                            checked={settings.stripClientOnly}
                            onChange={(v) => void patch({ stripClientOnly: v })}
                        />
                    </SettingRow>
                </Section>

                {/* ---- 存储与缓存 ---- */}
                <Section title="存储与缓存">
                    <SettingRow label="工作缓存目录" desc="下载缓存、解包与构建中间产物都在这里">
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
                            className={STILL_HOVERABLE}
                            disabled={!!cleaning}
                            onClick={() => readUsage()}
                        />
                        <Btn
                            size="sm"
                            className={STILL_HOVERABLE}
                            disabled={!usage || usage.staleCount === 0 || !!cleaning}
                            title={
                                usage?.staleCount
                                    ? `删掉 ${usage.staleDays} 天没再用到过的文件；下次转换遇到它们会重新联网下载`
                                    : "没有满足条件的文件"
                            }
                            onClick={() => void runClean("stale")}
                        >
                            {cleaning === "stale" ? "清理中…" : "清过期"}
                        </Btn>
                        <Btn
                            size="sm"
                            variant="danger"
                            className={STILL_HOVERABLE}
                            disabled={
                                !usage || usage.filesCount === 0 || !!cleaning || usage.busy
                            }
                            title={
                                usage?.busy
                                    ? "转换进行中：缓存里的文件正在被这次转换使用，先结束或等它跑完"
                                    : "删掉全部下载缓存；下次转换所有模组都要重新联网下载"
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
                                  ? "没有残留：半截下载、崩溃留下的暂存目录、空壳目录都是 0"
                                  : `${junkBytes(usage) ? `${formatSize(junkBytes(usage))}：` : ""}半截下载 ${usage.partsCount} 个 · 残留暂存 ${usage.orphanCount} 个 · 空壳目录 ${usage.emptyDirs} 个` +
                                    (usage.busy ? "｜转换进行中，正在写的半截下载不计入" : "")
                        }
                    >
                        <Btn
                            size="sm"
                            icon={Trash2}
                            className={STILL_HOVERABLE}
                            disabled={!usage || !!cleaning}
                            title="删下载写坏的半截文件、上次崩溃留下的暂存目录、清完剩下的空目录；不碰下载缓存，转换中也能点"
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
                        label="构建后自检"
                        desc="打包完成后离线对账产物：模组是否齐、jar 是否完整、依赖是否被误剔；不启动服务端"
                    >
                        <Toggle
                            size="md"
                            checked={settings.verifyAfterBuild}
                            onChange={(v) => void patch({ verifyAfterBuild: v })}
                        />
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
                        label="版本"
                        descMono
                        desc={`v${api.APP_VERSION} · build 3 · Tauri 2`}
                    >
                        <Btn
                            size="sm"
                            icon={ExternalLink}
                            className="w-8 px-0"
                            title="在浏览器中打开仓库"
                            onClick={() => void api.openExternal(api.REPO_URL)}
                        />
                        <Btn size="sm" icon={RefreshCw} onClick={() => void checkUpdate()}>
                            {UPDATE_LABEL[update]}
                        </Btn>
                    </SettingRow>
                </Section>
            </div>
        </div>
    );
}

/** 分组：等宽小标题 + 无内边距卡片（行间 1px $stroke-soft 分隔） */
function Section({ title, children }: { title: string; children: ReactNode }) {
    return (
        <section className="flex w-full flex-col gap-2">
            <SectionTitle>{title}</SectionTitle>
            <Panel gap={0} className="divide-y divide-stroke-soft p-0">
                {children}
            </Panel>
        </section>
    );
}

/** 读设置的往返期间占位：按真实分组与行数排，免得整页先白一下再蹦出内容 */
function SettingsSkeleton() {
    const groups: Array<[string, number]> = [
        ["转换选项", 2],
        ["存储与缓存", 3],
        ["网络", 4],
        ["外观与关于", 2],
    ];
    return (
        <div className="flex flex-col gap-5">
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

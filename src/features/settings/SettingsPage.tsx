/**
 * 设置页 Settings（SS.pen `F4na6`）
 *
 * 三个分组，每组 = 等宽小标题（11/600，字距 1.2）+ 一张无内边距卡片，
 * 行与行之间用 1px $stroke-soft 分隔（divide-y），行标尺 padding[14,20]。
 *  - 转换选项：服务端输出目录 / 工作缓存目录 / 剔除客户端专属资源 / 联网反查端信息 / 构建后自动校验
 *  - 网络：下载源 / 并发下载数
 *  - 外观与关于：主题（三态分段）/ 版本（仓库外链 + 检查更新）
 * 读写走 @/lib/api 门面；主题走 @/lib/theme 单一真源（侧栏按钮同步）。
 * 设置是「改一处即持久化」，所以写盘失败必须外显（否则界面显示已生效、重启又回退），
 * 失败后从后端重读一次，让界面与真正常量的那份一致。
 */
import { ExternalLink, Folder, FolderOpen, Monitor, Moon, RefreshCw, Sun } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import * as api from "@/lib/api";
import { notify } from "@/lib/notify";
import { useTheme, type Theme } from "@/lib/theme";
import type { AppSettings } from "@/lib/types";
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

export function SettingsPage() {
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [sources, setSources] = useState<SelectOption[]>([]);
    const [theme, setTheme] = useTheme();
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

    /** 在资源管理器里打开目录：静默失败会被当成「按钮坏了」，一律外显 */
    const revealDir = async (dir: string, label: string) => {
        if (!dir.trim()) {
            notify(`${label}还没设置，先点「选择」指定一个目录`, "warn");
            return;
        }
        try {
            await api.openDir(dir);
        } catch (e) {
            notify(`打开${label}失败：${errOf(e)}`, "error");
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
            <PageHeader compact title="设置" sub="转换行为、网络与外观偏好" />

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
                        <IconBtn
                            icon={FolderOpen}
                            title="在资源管理器中打开"
                            onClick={() => void revealDir(settings.outputDir, "输出目录")}
                        />
                        <Btn size="sm" onClick={() => void pickDir("outputDir")}>
                            选择
                        </Btn>
                    </SettingRow>
                    <SettingRow label="工作缓存目录" desc="解析与构建的临时文件存放位置">
                        <TextInput
                            plain
                            icon={Folder}
                            readOnly
                            value={settings.cacheDir}
                            onClick={() => void pickDir("cacheDir")}
                            className="w-[300px] cursor-pointer"
                        />
                        <IconBtn
                            icon={FolderOpen}
                            title="在资源管理器中打开"
                            onClick={() => void revealDir(settings.cacheDir, "缓存目录")}
                        />
                        <Btn size="sm" onClick={() => void pickDir("cacheDir")}>
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
                </Section>

                {/* ---- 网络 ---- */}
                <Section title="网络">
                    <SettingRow
                        label="下载源"
                        desc="版本表与加载器 jar 优先走国内镜像，不通自动回落官方（模组文件在 Modrinth，没有镜像）"
                    >
                        <SearchSelect
                            plain
                            value={settings.downloadSource}
                            options={sources}
                            onChange={(v) => void patch({ downloadSource: v as AppSettings["downloadSource"] })}
                            className="w-[280px]"
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
                    <SettingRow label="主题" desc="深色为默认，跟随系统切换">
                        <SegTabs items={THEME_TABS} value={theme} onChange={setTheme} />
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
        ["转换选项", 4],
        ["网络", 3],
        ["外观与关于", 2],
    ];
    return (
        <div className="flex flex-col gap-5">
            <PageHeader compact title="设置" sub="转换行为、网络与外观偏好" />
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

/**
 * 设置页 Settings（SS.pen `F4na6`）
 *
 * 三个分组，每组 = 等宽小标题（11/600，字距 1.2）+ 一张无内边距卡片，
 * 行与行之间用 1px $stroke-soft 分隔（divide-y），行标尺 padding[14,20]。
 *  - 转换选项：工作缓存目录 / 剔除客户端专属资源 / 构建后自动校验
 *  - 网络：下载源 / 并发下载数
 *  - 外观与关于：主题（三态分段）/ 版本（仓库外链 + 检查更新）
 * 读写走 @/lib/api 门面；主题走 @/lib/theme 单一真源（侧栏按钮同步）。
 */
import { ExternalLink, Folder, Monitor, Moon, RefreshCw, Sun } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import * as api from "@/lib/api";
import { useTheme, type Theme } from "@/lib/theme";
import type { AppSettings } from "@/lib/types";
import {
    Btn,
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
} from "@/components/design/ui";

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

export function SettingsPage() {
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [sources, setSources] = useState<SelectOption[]>([]);
    const [theme, setTheme] = useTheme();
    const [update, setUpdate] = useState<UpdateState>("idle");

    useEffect(() => {
        void api.getSettings().then(setSettings);
        void api
            .listDownloadSources()
            .then((list) => setSources(list.map(({ value, label, recommended }) => ({ value, label, recommended }))));
    }, []);

    /** 局部更新 + 立即持久化 */
    const patch = (p: Partial<AppSettings>) => {
        setSettings((s) => {
            if (!s) return s;
            const next = { ...s, ...p };
            void api.saveSettings(next);
            return next;
        });
    };

    const pickCacheDir = async () => {
        const dir = await api.pickDirectory();
        if (dir) patch({ cacheDir: dir });
    };

    const checkUpdate = async () => {
        setUpdate("checking");
        const latest = await api.checkUpdate();
        setUpdate(latest === api.APP_VERSION ? "latest" : "available");
        window.setTimeout(() => setUpdate("idle"), 3000);
    };

    if (!settings) return null;

    return (
        <div className="flex flex-col gap-5">
            <PageHeader compact title="设置" sub="转换行为、网络与外观偏好" />

            <div className="flex w-full flex-col gap-5">
                {/* ---- 转换选项 ---- */}
                <Section title="转换选项">
                    <SettingRow label="工作缓存目录" desc="解析与构建的临时文件存放位置">
                        <TextInput
                            plain
                            icon={Folder}
                            readOnly
                            value={settings.cacheDir}
                            onClick={() => void pickCacheDir()}
                            className="w-[300px] cursor-pointer"
                        />
                        <Btn size="sm" onClick={() => void pickCacheDir()}>
                            选择
                        </Btn>
                    </SettingRow>
                    <SettingRow
                        label="剔除客户端专属资源"
                        desc="移除光影、小地图、键盘鼠标等 41 类已知客户端模组"
                    >
                        <Toggle
                            size="md"
                            checked={settings.stripClientOnly}
                            onChange={(v) => patch({ stripClientOnly: v })}
                        />
                    </SettingRow>
                    <SettingRow label="构建后自动校验" desc="生成前启动一次服务端空跑，验证依赖完整性">
                        <Toggle
                            size="md"
                            checked={settings.verifyAfterBuild}
                            onChange={(v) => patch({ verifyAfterBuild: v })}
                        />
                    </SettingRow>
                </Section>

                {/* ---- 网络 ---- */}
                <Section title="网络">
                    <SettingRow label="下载源" desc="服务端核心与模组依赖的 Maven 镜像">
                        <SearchSelect
                            plain
                            value={settings.downloadSource}
                            options={sources}
                            onChange={(v) => patch({ downloadSource: v as AppSettings["downloadSource"] })}
                            className="w-[280px]"
                        />
                    </SettingRow>
                    <SettingRow label="并发下载数" desc="同时拉取依赖 jar 的线程数（1–16）">
                        <Stepper
                            plain
                            min={1}
                            max={16}
                            value={settings.concurrency}
                            onChange={(v) => patch({ concurrency: v })}
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

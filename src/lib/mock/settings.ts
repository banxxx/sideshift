/** 设置（浏览器 dev 兜底）：localStorage 持久化，真实实现走 Rust */
import type { AppSettings, VersionOption } from "@/lib/types";

const SETTINGS_KEY = "sideshift.settings";

export const mockDefaultSettings: AppSettings = {
    outputDir: "~/Documents/SideShift/output",
    cacheDir: "D:\\SideShift\\cache",
    stripClientOnly: true,
    autoClassifyOnline: true,
    verifyAfterBuild: false,
    downloadSource: "official",
    concurrency: 6,
};

/** 下载源下拉（Settings · 网络）：与 Rust `list_download_sources` 同序同文案 */
export const mockDownloadSources: VersionOption[] = [
    { value: "official", label: "官方源", recommended: true },
    { value: "bmclapi", label: "BMCLAPI 国内镜像" },
];

export function mockLoadSettings(): AppSettings {
    try {
        const raw = localStorage.getItem(SETTINGS_KEY);
        return raw ? { ...mockDefaultSettings, ...JSON.parse(raw) } : mockDefaultSettings;
    } catch {
        return mockDefaultSettings;
    }
}

export function mockSaveSettings(s: AppSettings): void {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
}

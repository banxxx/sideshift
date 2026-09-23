/** 设置（浏览器 dev 兜底）：localStorage 持久化，真实实现走 Rust */
import type { AppSettings, VersionOption } from "@/lib/types";

const SETTINGS_KEY = "sideshift.settings";

/**
 * 默认目录对与 Rust 侧同构：`{数据根}\{output,cache}`。
 * 真实数据根由 Rust 决定（便携包 → exe 同级 data；安装器指定 → 那个根；否则预选非系统盘），
 * 浏览器 dev 没这些概念，写死这台机器常见的 D: 只为让界面有东西可显示。
 */
export const mockDefaultSettings: AppSettings = {
    outputDir: "D:\\SideShift\\output",
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

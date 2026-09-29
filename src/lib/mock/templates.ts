/** 转换模板（浏览器 dev 兜底）：localStorage 持久化，真实实现走 Rust 的配置目录 */
import type { ConversionTemplate, TemplateValues } from "@/lib/types";
import { mockLoadSettings } from "./settings";

const TEMPLATES_KEY = "sideshift.templates";

/**
 * 新建模板的种子值（浏览器 dev）：逐档抄 Rust `ConversionOptions::default()`，
 * 只有「本机安装 Loader」跟着这份 mock 设置走——真实命令也是这么取的（`template_defaults`）。
 * 这里改了就得同步改那边，两份字面量对不上时浏览器里看到的是假默认。
 */
export function mockTemplateDefaults(): TemplateValues {
    return {
        installLoaderLocally: mockLoadSettings().installLoaderLocally,
        generateScripts: true,
        javaPath: "",
        outputOverride: "",
        memoryMb: 4096,
        nogui: true,
        agreeEula: true,
        useAikarFlags: true,
        extraJvmArgs: "",
        gamemode: "survival",
        difficulty: "easy",
        serverPort: 25565,
        maxPlayers: 20,
        motd: "A Minecraft server",
        levelSeed: "",
        onlineMode: true,
    };
}

/** 三条样例（与设计稿屏 ① 同一批）：让「排序 / 套用 / 漂移 / 删除」四条链在浏览器里都能走一遍 */
const mockTemplates: ConversionTemplate[] = [
    {
        id: "tpl-survival",
        name: "生存服预设",
        note: "每周六开 · 结束后不停服",
        updatedAt: 0,
        values: {
            installLoaderLocally: true,
            generateScripts: true,
            memoryMb: 8192,
            nogui: true,
            agreeEula: true,
            useAikarFlags: true,
            gamemode: "survival",
            difficulty: "normal",
            maxPlayers: 20,
            motd: "生存服 · 周末开",
            onlineMode: true,
        },
    },
    {
        id: "tpl-friends",
        name: "朋友联机临时服",
        note: "临时开两小时 · 下班就关",
        updatedAt: 0,
        values: {
            generateScripts: true,
            memoryMb: 4096,
            nogui: true,
            agreeEula: true,
            gamemode: "survival",
            difficulty: "easy",
            onlineMode: false,
        },
    },
    {
        id: "tpl-smoke",
        name: "轻量测试服",
        note: "只测能不能起 · 不建档",
        updatedAt: 0,
        values: {
            generateScripts: true,
            memoryMb: 2048,
            agreeEula: true,
        },
    },
];

export function mockListTemplates(): ConversionTemplate[] {
    try {
        const raw = localStorage.getItem(TEMPLATES_KEY);
        // 没存过 ⇒ 给样例（首次进浏览器 dev 就看一张空表的话，列表/下拉/漂移三条链都演不了）
        return raw ? (JSON.parse(raw) as ConversionTemplate[]) : mockTemplates;
    } catch {
        return mockTemplates;
    }
}

export function mockSaveTemplates(list: ConversionTemplate[]): void {
    localStorage.setItem(TEMPLATES_KEY, JSON.stringify(list));
}

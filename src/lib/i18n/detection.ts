/**
 * 语言档位与「跟随系统」的解析（i18n 的唯一判据源）。
 *
 * 两码一套：`AppLocale` 是**线上档位**（写进 settings.json 的用户选择，含 `auto`），
 * `Locale` 是**生效档位**（i18next 的 lng，只有三种）。`auto` 永远不会成为生效档位。
 *
 * 界面上一律显示语言的**自称**（简体中文 / 繁體中文 / English）——这条是行业惯例：
 * 用户在自己的语言里才认得出它，所以这几个名字**不进翻译目录**，切换失败也不会看不见。
 */

import type { AppLocale } from "@/lib/types";

/** 线上档位的唯一声明在 `@/lib/types`（与 Rust `AppSettings.locale` 逐字对齐），这里只转发 */
export type { AppLocale };

/** 生效档位（i18next 的 lng，同时是 resources/ 下的目录名） */
export type Locale = "zh-CN" | "zh-TW" | "en-US";

/** 兜底档：简体中文是这套目录的源语言，界面里那句原文就写在调用点 */
export const FALLBACK_LOCALE: Locale = "zh-CN";

/** localStorage 镜像键：冷启动第一帧就要知道语言，等不到异步的 get_settings()。
 *  它只是**加速件**，不是准绳——准绳是 settings.json 里那一档，`initI18n` 挂树后会拿它对账并把这里补正 */
export const CHOICE_KEY = "locale";

export const WIRE_TO_LOCALE: Record<Exclude<AppLocale, "auto">, Locale> = {
    zhCn: "zh-CN",
    zhTw: "zh-TW",
    enUs: "en-US",
};

export const LOCALE_TO_WIRE: Record<Locale, Exclude<AppLocale, "auto">> = {
    "zh-CN": "zhCn",
    "zh-TW": "zhTw",
    "en-US": "enUs",
};

/** 原文里有 `zh` / `zh_CN` / `zh-Hant-TW` 几种写法；`_` 是 Windows 侧的常见形态 */
const EXACT: Record<string, Locale> = {
    "zh-cn": "zh-CN",
    "zh-hans": "zh-CN",
    "zh-sg": "zh-CN",
    "zh-tw": "zh-TW",
    "zh-hk": "zh-TW",
    "zh-mo": "zh-TW",
    "zh-hant": "zh-TW",
    en: "en-US",
    "en-us": "en-US",
};

/**
 * BCP-47 → 三种支持档之一（认不出返回 undefined，让调用方继续往下试下一个 tag）。
 * 先看次语言标签（Hant / TW / HK / MO）再看 `zh` 前缀：繁体标记比前缀更准。
 * 非中文一律落英文：这套目录只有中文与英文两摊，其他语种的用户读英文比读简体靠谱。
 */
export function matchLocale(raw: string | undefined | null): Locale | undefined {
    if (!raw) return undefined;
    const tag = raw.trim().toLowerCase().replace(/_/g, "-");
    if (EXACT[tag]) return EXACT[tag];
    if (/^zh-(tw|hk|mo)/.test(tag) || tag.includes("hant")) return "zh-TW";
    if (tag.startsWith("zh")) return "zh-CN";
    if (tag.startsWith("en")) return "en-US";
    return undefined;
}

/** 系统语言（webview 给的 navigator.languages 按偏好排序，取第一个认得出的） */
export function systemLocale(): Locale {
    for (const tag of navigator.languages ?? []) {
        const hit = matchLocale(tag);
        if (hit) return hit;
    }
    return matchLocale(navigator.language) ?? FALLBACK_LOCALE;
}

/** 用户选择 → 生效档位。`auto`/无效值（旧版本写错的、手改出来的）都走系统判定，不报错 */
export function resolveLocale(choice: string | null | undefined): Locale {
    if (choice && choice !== "auto" && choice in WIRE_TO_LOCALE) {
        return WIRE_TO_LOCALE[choice as Exclude<AppLocale, "auto">];
    }
    return systemLocale();
}

/**
 * 设置页那四档。`label` 为空 = 语言自称，按原样显示、不翻（见文件头）；
 * 「跟随系统」那句是普通文案，走目录（`label` 给了原文，调用方 `t(label)`）。
 */
export const LOCALE_OPTIONS: Array<{ value: AppLocale; label: string }> = [
    { value: "auto", label: "跟随系统" },
    { value: "zhCn", label: "简体中文" },
    { value: "zhTw", label: "繁體中文" },
    { value: "enUs", label: "English" },
];

/** 读镜像（隐私模式下 localStorage 会抛，读不到就当没选过 → 走系统判定） */
export function readChoice(): AppLocale | null {
    try {
        const v = localStorage.getItem(CHOICE_KEY);
        return v === "auto" || v === "zhCn" || v === "zhTw" || v === "enUs" ? v : null;
    } catch {
        return null;
    }
}

export function writeChoice(choice: AppLocale | null): void {
    try {
        if (choice === null) localStorage.removeItem(CHOICE_KEY);
        else localStorage.setItem(CHOICE_KEY, choice);
    } catch {
        /* 存不下也要能切语言：镜像只是让冷启动少等一次 IPC */
    }
}

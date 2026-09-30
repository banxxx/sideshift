/**
 * i18n 装配（全站唯一入口）：`t(键, 简体中文原文)` —— 第二参是内联原文、当 defaultValue；
 * zh-CN 因此没有目录文件，缺译一律掉回中文而不是漏键。动态句走 `tSource`/`backendText`
 * 查 `source-keys.ts` 的「中文 → 键」表（`scripts/i18n.mjs sync` 生成，别手改），查不到原样返回中文。
 * 硬约束：改了内联中文必须跑 `pnpm i18n:lock`，否则 check 报 STALE；键不许随手改名。
 * 组件里一律 `useT()`，纯函数模块用 `t` 但禁止模块顶层求值；可数数量用 `count` 槽；数字/日期走 `Intl`。
 */
import i18next from "i18next";
import { initReactI18next, useTranslation } from "react-i18next";
import { SOURCE_KEYS } from "./source-keys";
import {
    FALLBACK_LOCALE,
    readChoice,
    resolveLocale,
    writeChoice,
    type AppLocale,
    type Locale,
} from "./detection";

/** 一个语言档 = resources/<locale>/<area>.json，加载时摊平成一张 translation 表 */
const files = import.meta.glob("./resources/*/*.json", {
    eager: true,
    import: "default",
}) as Record<string, Record<string, string>>;

/** 同一档里跨 shard 撞键 = 一个键被写进两个文件（挪 area 时漏删旧的）；取先到的并让 check 报出来 */
const resources: Record<string, { translation: Record<string, string> }> = {};
for (const [path, dict] of Object.entries(files)) {
    const lng = /\/resources\/([^/]+)\//.exec(path)?.[1];
    if (!lng) continue;
    const bucket = (resources[lng] ??= { translation: {} });
    for (const [key, value] of Object.entries(dict)) {
        if (key in bucket.translation && bucket.translation[key] !== value) {
            console.warn(`[i18n] ${lng} 同键两译，取先到的那条：${key}`);
            continue;
        }
        bucket.translation[key] = value;
    }
}

export type { TFunction } from "i18next";
export {
    FALLBACK_LOCALE,
    LOCALE_OPTIONS,
    LOCALE_TO_WIRE,
    WIRE_TO_LOCALE,
    matchLocale,
    readChoice,
    resolveLocale,
    systemLocale,
} from "./detection";
export type { AppLocale, Locale } from "./detection";
export { LocaleGate } from "./LocaleGate";
export { useTranslation };

/**
 * 首帧语言与装配。`main.tsx` 在挂树之前 await 它，
 * 这样第一帧就是最终语言，不会先闪一次中文再换过去。
 *
 * 生效语言先由 localStorage 镜像同步定（不等 IPC），然后再拿设置对一次账。
 * **磁盘那份是准绳，镜像只是冷启动少等一次 IPC 的加速件**：两者一旦不符（上一次写过盘却没切成、
 * 写盘失败、或被别的整份回写挤掉过），按设置走并把镜像补正过来。
 * 只在镜像为空时才读设置是不够的——那份偏差会一直活到下一次改语言，表现就是「界面语言与设置里
 * 选的那档不一样，重启也不变」。一致时 `resolveLocale` 相等 ⇒ 一次 `changeLanguage` 都不发，
 * 正常路径不会多那一闪。
 *
 * 读设置由调用方以回调传进来，不在这里 import `@/lib/api/settings`：那条线一旦是静态的就成了
 * `i18n → api → lib/format → i18n` 的环（`lib/*` 里的纯函数文案后面都要用 `t`），
 * 而写成 `await import()` 会被打包器报「动态导入无效」（同一 chunk）。
 */
export async function initI18n(
    loadSavedChoice?: () => Promise<AppLocale | null | undefined>
): Promise<void> {
    await i18next.use(initReactI18next).init({
        resources,
        lng: resolveLocale(readChoice()),
        fallbackLng: FALLBACK_LOCALE,
        // 键名里的 `.` 只是分组用的字面字符：不许按分隔符拆成嵌套路径（`settings.a.b` 是一个键）
        keySeparator: false,
        nsSeparator: false,
        load: "currentOnly",
        // 资源是打进 bundle 的本地 JSON；`initAsync:false` 才让 init 不走 setTimeout 那一岔
        // （i18next 26 把 v23 的 `initImmediate` 反过来命名成了这个）
        initAsync: false,
        returnNull: false,
        // 插值一律不转义：界面走 React 的文本节点（不是 innerHTML），转义只会把
        // `Pierre's Mod` 显示成 `Pierre&#39;s Mod`。改造前这些句子是模板字符串拼的，本来不转义
        interpolation: { escapeValue: false },
    });
    document.documentElement.lang = activeLocale();
    if (loadSavedChoice) void reconcileWithSavedChoice(loadSavedChoice);
}

/**
 * 拿设置里那档对一次账（`initI18n` 挂树后异步跑；导出只为让冒烟能直接演这一条规则）。
 * 判据是**生效档**而不是档位串：`auto` 与它解析出来的那三种之一本就该相等，
 * 相等时一趟 `changeLanguage` 都不发，视图不会被重挂。
 */
export async function reconcileWithSavedChoice(
    load: () => Promise<AppLocale | null | undefined>
): Promise<void> {
    try {
        const saved = await load();
        if (saved && resolveLocale(saved) !== i18next.language) await applyLocaleChoice(saved);
    } catch {
        /* 设置读不到就用镜像/系统语言，不值得为这个打断启动 */
    }
}

/** 组件内的翻译入口（react-i18next 订了 languageChanged，切语言自动重渲染） */
export function useT() {
    return useTranslation().t;
}

/**
 * 给非组件模块用的翻译入口（组件内用 `useT`，理由见文件头）。
 *
 * 第二参可以是中文原文（字符串）也可以是插值参数（对象），三种写法都收：
 * `t(k, "中文")` / `t(k, "中文", { n })` / `t(k, { n })`（最后一种没有原文，check 会报 NEEDS-ZH）。
 * 位置参数在这里折成 `{ defaultValue, ...options }`——i18next 两种形态等价（实测过 `count` 在
 * 对象形态下照样挑对 `_one`/`_other`），折成对象省掉三重载，`useT()` 那条走 i18next 自己的重载。
 */
export function t(key: string, options?: Record<string, unknown>): string;
export function t(key: string, defaultValue: string, options?: Record<string, unknown>): string;
export function t(
    key: string,
    defaultValueOrOptions?: string | Record<string, unknown>,
    options?: Record<string, unknown>
): string {
    // 分两岔而不是把位置参数折成 `{ defaultValue }` 再单调一次：那样传进去的是联合类型，
    // i18next 那串重载签名接不住（TS 会在 overload 之间来回判失败）。两条路都实测过等价。
    if (typeof defaultValueOrOptions === "string")
        return i18next.t(key, defaultValueOrOptions, options ?? {}) as string;
    return i18next.t(key, defaultValueOrOptions ?? {}) as string;
}

/**
 * 「表建在函数里」那批小工厂收的 t（见各处注释：顶层建表会把词冻在首次加载的语言上）。
 *
 * 就是 `useT()` 的形状，直接取而不自己描一个 `(key, zh?) => string`：i18next 的 `TFunction` 是重载
 * 泛型，任何「更宽」的手写签名都装不下它（试过的报错是「Source provides no match for required
 * element at position 1」）。纯函数模块不经过这条线——它们直接用本文件的 `t` 就地建表。
 */
export type TranslateFn = ReturnType<typeof useT>;

/**
 * 运行时才知道内容的中文 → 译文（后端整句、store 里的原文、下拉 label）。
 *
 * 调用点给不出字面量原文，所以原文由参数本身当 `defaultValue`：查 `source-keys.ts` 命中键就用目录，
 * 没命中（这句还没登记、或后端加了新句）就原样返回——**表现是这一条没翻，不是界面坏掉**。
 */
export function tSource(raw: string | null | undefined): string {
    if (!raw) return "";
    const key = SOURCE_KEYS[raw];
    return key ? (i18next.t(key, { defaultValue: raw }) as string) : raw;
}


/** 当前生效档 */
export function activeLocale(): Locale {
    return i18next.language as Locale;
}

/** Intl 用的 locale 串（数字分组、日期格式、相对时间都吃这个） */
export function intlLocale(): string {
    return i18next.language || FALLBACK_LOCALE;
}

/**
 * 切语言。`choice` 是设置里那档（auto / zhCn / …），生效档由 `resolveLocale` 算。
 * 磁盘上的设置由调用方存（设置页那条链路），这里只管当前进程 + localStorage 镜像。
 */
export async function applyLocaleChoice(choice: AppLocale | null): Promise<Locale> {
    writeChoice(choice);
    const lng = resolveLocale(choice);
    if (i18next.language !== lng) await i18next.changeLanguage(lng);
    document.documentElement.lang = lng;
    return lng;
}

/** 订一次语言切换（`LocaleGate` 用；组件里通常不用，`useT` 已经订了） */
export function onLanguageChanged(fn: (lng: Locale) => void): () => void {
    const handler = () => fn(activeLocale());
    i18next.on("languageChanged", handler);
    return () => i18next.off("languageChanged", handler);
}

/**
 * 后端模板句的翻译入口。
 *
 * 后端为了一句能翻又留得住中文，同时发**带 `{{slot}}` 的中文模板**（`Msg.key`）、**参数**
 * （`Msg.args`）和**已经渲染好的中文整句**（`Msg.zh`，日志/剪贴板/兜底都要它）。
 * 这里拿模板查 `source-keys.ts` 得到语义键，再用 `zh` 当 `defaultValue` 去要当前档的那句话：
 * 目录有译文 ⇒ 用译文渲染；没有 ⇒ 直接给后端算好的中文。
 *
 * `template` 省略 = 这一条后端还没接模板（只剩整句），那也走同一张表按整句查。
 */
export function backendText(
    raw: string | null | undefined,
    template?: string | null,
    args?: Record<string, unknown> | null,
    translate: (key: string, options?: Record<string, unknown>) => string = t
): string {
    const zh = template ?? raw;
    if (!zh) return "";
    const key = SOURCE_KEYS[zh];
    if (!key) return raw ?? zh;
    const out = translate(key, { ...(args ?? {}), defaultValue: raw ?? zh });
    // 参数没给全时若查出带槽的串（旧数据、或目录被改过），别把 `{{}}` 露给用户
    return out.includes("{{") ? (raw ?? out) : out;
}

/** 组件内的后端句子翻译（跟着语言切换重渲染） */
export function useBackendText() {
    const { t: tx } = useTranslation();
    return (
        raw: string | null | undefined,
        key?: string | null,
        args?: Record<string, unknown> | null
    ) => backendText(raw, key, args, (k, o) => tx(k, o));
}

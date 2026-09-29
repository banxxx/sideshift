/**
 * 转换模板：`ConversionOptions` 里「换个包还要一样」那 16 档的一份快照。
 *
 * 字段范围（对着 `ConversionOptions` 逐档过，19 档收 16）：
 *  - 收：运行环境 4（本机装 Loader / 生成启动脚本 / 本地 Java / 输出目录覆写）·
 *    启动参数 5（内存 / nogui / eula / Aikar / 附加 JVM）· 服务端设置 7（模式 / 难度 / 端口 / 人数 / MOTD / 种子 / 正版验证）
 *  - 不收 3 档的理由各不相同：`mcVersion`/`loaderVersion` 属于**这个包**（换包必然不同，进了模板就是拿旧包的答案
 *    去覆盖新包的检测结论）；`javaVersion` 是 MC 版本推出来的需求线，不是"要装哪版"，存进模板等于冻结一次已过期的推导；
 *    `keepDirs`/`keepFiles` 是包内相对路径，换包后指的东西根本不存在，而且勾了包根同名文件会让设置项让位（`emit_root`），
 *    模板带它就是在播报可能没进产物的配置。方案（哪些模组剔除/保留）与 `allowMissingMods` 同理不进——
 *    前者是这一包的清单，后者是对这一次缺件的知情同意，都不该跨包复用。
 *
 * `TemplateValues` 用 `Partial`（字段**缺席 = 这档不在模板里**）而不是并列一份"勾了哪些"的数组：
 * 套用就是逐键抄写这里有的那些，转换页其余字段保持原值，两个真源不会分叉。
 */
import { t } from "@/lib/i18n";
import type { ConversionOptions } from "./options";

/** 模板能收的字段：键名一律取 `ConversionOptions` 的字段名，套用因此不需要映射表 */
export type TemplateFieldKey =
    | "installLoaderLocally"
    | "generateScripts"
    | "javaPath"
    | "outputOverride"
    | "memoryMb"
    | "nogui"
    | "agreeEula"
    | "useAikarFlags"
    | "extraJvmArgs"
    | "gamemode"
    | "difficulty"
    | "serverPort"
    | "maxPlayers"
    | "motd"
    | "levelSeed"
    | "onlineMode";

/** 模板里的值：出现的键才写入转换页 */
export type TemplateValues = Partial<Pick<ConversionOptions, TemplateFieldKey>>;

export interface ConversionTemplate {
    /** 稳定 id（不翻、不随改名变）；转换页的「已套用」认的是它 */
    id: string;
    name: string;
    /** 备注：只在列表卡那一行露个脸（转换页那颗下拉只报名字），不参与套用 */
    note: string;
    values: TemplateValues;
    /** 最后保存的时刻（epoch 毫秒） */
    updatedAt: number;
}

export type TemplateGroup = "runtime" | "launch" | "server";

/**
 * 编辑器里一行的控件形状：编辑页按它挑控件，计数与灰化只认 `key`。
 * `path` 单列一档而不是复用 `text`：那一格不给打字、点了开系统目录选择器，
 * 空串另有读数（「跟随全局」而不是留白）——和 `javaPath` 是同一族（绑本机路径、换机各自回落）。
 */
export type TemplateFieldKind = "toggle" | "stepper" | "select" | "text" | "number" | "path";

export interface TemplateFieldMeta {
    key: TemplateFieldKey;
    group: TemplateGroup;
    kind: TemplateFieldKind;
    /** 字段行上的全称（与转换页同一句 ⇒ 直接复用 `convert.*` 键，译文已有） */
    label: string;
    /** 漂移读数里用的短名（那一行只有几十 px，全称念完就断了） */
    short: string;
    /** 绑本机路径的那两档要说清回落口径：换机/换目录之后这一项会退成什么 */
    hint?: string;
}

/** 16 档的显示名表（表建在函数里、每格一条 `t(字面量)`：模块顶层求值会把词冻在首次加载的语言上） */
export function templateFields(): TemplateFieldMeta[] {
    return [
        {
            key: "installLoaderLocally",
            group: "runtime",
            kind: "toggle",
            label: t("convert.install-loader", "本机安装 Loader（产物上传即开服）"),
            short: t("templates.local-loader", "本机装 Loader"),
        },
        {
            key: "generateScripts",
            group: "runtime",
            kind: "toggle",
            label: t("convert.generate-start", "生成启动脚本（start.sh / start.bat）"),
            short: t("templates.start-scripts", "启动脚本"),
        },
        {
            key: "javaPath",
            group: "runtime",
            kind: "select",
            label: t("convert.local-java", "本地 Java 环境"),
            short: t("templates.java", "Java"),
            hint: t("templates.falls-back-auto", "换机回落「自动选择」"),
        },
        {
            key: "outputOverride",
            group: "runtime",
            kind: "path",
            label: t("templates.output-dir", "输出目录（本次覆写）"),
            short: t("templates.output", "输出目录"),
            hint: t("templates.falls-back-global", "换机回落全局设置"),
        },
        {
            key: "memoryMb",
            group: "launch",
            kind: "stepper",
            label: t("convert.max-server", "服务器内存上限"),
            short: t("templates.memory", "内存"),
        },
        {
            key: "nogui",
            group: "launch",
            kind: "toggle",
            label: t("convert.start-headless", "无界面模式启动（--nogui）"),
            short: "nogui",
        },
        {
            key: "agreeEula",
            group: "launch",
            kind: "toggle",
            label: t("convert.write-eula", "自动写入 eula=true（同意 Mojang EULA）"),
            short: "EULA",
        },
        {
            key: "useAikarFlags",
            group: "launch",
            kind: "toggle",
            label: t("convert.aikar-flags", "Aikar's flags 优化参数组（G1GC 推荐）"),
            short: "Aikar",
        },
        {
            key: "extraJvmArgs",
            group: "launch",
            kind: "text",
            label: t("convert.extra-jvm", "附加 JVM 参数"),
            short: t("templates.extra-jvm", "附加 JVM"),
        },
        {
            key: "gamemode",
            group: "server",
            kind: "select",
            label: t("convert.game-mode", "游戏模式"),
            short: t("templates.game-mode", "游戏模式"),
        },
        {
            key: "difficulty",
            group: "server",
            kind: "select",
            label: t("convert.difficulty", "难度"),
            short: t("templates.difficulty", "难度"),
        },
        {
            key: "serverPort",
            group: "server",
            kind: "number",
            label: t("convert.server-port", "服务器端口"),
            short: t("templates.port", "端口"),
        },
        {
            key: "maxPlayers",
            group: "server",
            kind: "number",
            label: t("convert.max-players", "最大人数"),
            short: t("templates.players", "人数"),
        },
        {
            key: "motd",
            group: "server",
            kind: "text",
            label: t("convert.server-description", "服务器描述（MOTD）"),
            short: "MOTD",
        },
        {
            key: "levelSeed",
            group: "server",
            kind: "text",
            label: t("convert.world-seed", "世界种子（留空 = 随机生成）"),
            short: t("templates.seed", "种子"),
        },
        {
            key: "onlineMode",
            group: "server",
            kind: "toggle",
            label: t("convert.verified-accounts", "正版验证（online-mode）"),
            short: t("templates.online-mode", "正版验证"),
        },
    ];
}

/** 16 档的键（顺序同上）：编辑器初始化草稿、模板计数都按它走 */
export const TEMPLATE_FIELD_KEYS = [
    "installLoaderLocally",
    "generateScripts",
    "javaPath",
    "outputOverride",
    "memoryMb",
    "nogui",
    "agreeEula",
    "useAikarFlags",
    "extraJvmArgs",
    "gamemode",
    "difficulty",
    "serverPort",
    "maxPlayers",
    "motd",
    "levelSeed",
    "onlineMode",
] as const satisfies readonly TemplateFieldKey[];

export const TEMPLATE_FIELD_COUNT = TEMPLATE_FIELD_KEYS.length;

/** 一份模板收了几档（列表卡右侧那个「N 项」与摘要读数都问它） */
export const templateValueCount = (v: TemplateValues): number =>
    TEMPLATE_FIELD_KEYS.filter((k) => v[k] !== undefined).length;

/** 从整份选项里抄出模板能收的那 16 档（全部视为在模板内）：编辑器草稿与「存为模板」的种子走这一条 */
export function templateSeedOf(o: ConversionOptions): TemplateValues {
    const out: Record<string, unknown> = {};
    for (const k of TEMPLATE_FIELD_KEYS) out[k] = o[k];
    return out as TemplateValues;
}

/** 勾中的那几档才写进模板（未勾的值留在编辑器的草稿里，不落到存档） */
export function pickTemplateValues(
    draft: TemplateValues,
    ticked: Iterable<TemplateFieldKey>
): TemplateValues {
    const out: Record<string, unknown> = {};
    for (const k of ticked) if (draft[k] !== undefined) out[k] = draft[k];
    return out as TemplateValues;
}

/**
 * 名称与备注的输入上限（字符数，中英文同计数）。
 *
 * 定这两个数是为了让「省略号」有可预测的位置：卡片名与备注、下拉那一条、删除弹窗的副标都是按宽度截断的，
 * 不封顶的话长到几十上百字的名字会把这几处一律压成省略号，等于全都没名字。
 * 40 够写「生存服 20 人 · Aikar · 大内存」这一档描述；80 给备注留两倍行程（它是整句，不是标签）。
 */
export const TEMPLATE_NAME_MAX = 40;
export const TEMPLATE_NOTE_MAX = 80;

/**
 * 模板数量上限（2026-09-30 他点名设的 200）。
 *
 * 收在这一条而不是散进三个入口：新增模板的路径有**四条**（编辑页保存、另存为副本、列表页复制、
 * 以及将来可能加的导入），闸门只有一条写盘路（`useTemplateTable.commit`）⇒ 挡在那里，
 * 四条路一起生效，将来加第五条不用再想起来补。
 * 上限按**整表**算：删除永远放行，只有让它变长的提交才拦。
 */
export const TEMPLATE_MAX = 200;

/**
 * 让开同名位：名称是这一族界面认人的凭据（编辑页拿它拦重复），
 * 所以复制与另存为副本不能自己造出一枚同名 ⇒ 列表两行一样的名字、下拉两个一样的读数。
 *
 * 加「 副本」/「 2」这类后缀是**程序在写名字**，输入框上的 `maxLength` 管不到这里 ⇒ 上限在这一条里收：
 * 尾部不够放后缀就从名字头上削，削完再排号，绝不吐出一条超限的名字。
 */
export function uniqueTemplateName(want: string, taken: Iterable<string>): string {
    const used = new Set(taken);
    const base = want.slice(0, TEMPLATE_NAME_MAX);
    if (!used.has(base)) return base;
    for (let n = 2; ; n++) {
        const suffix = ` ${n}`;
        const candidate = `${base.slice(0, TEMPLATE_NAME_MAX - suffix.length)}${suffix}`;
        if (!used.has(candidate)) return candidate;
    }
}

/**
 * 套用过之后又被改动的字段（漂移判据：模板里有这一档，而转换页现在的值和模板给的不一样）。
 * 空数组 = 没改过，小卡那行说明走「已套用」那一句；非空则染金，读数要说"改过几项、哪几项"。
 */
export function driftedKeys(
    values: TemplateValues,
    options: ConversionOptions
): TemplateFieldKey[] {
    return TEMPLATE_FIELD_KEYS.filter(
        (k) => values[k] !== undefined && values[k] !== options[k]
    );
}

/** 在线添加与取证（本地 jar 取证 / 搜索 / 构建版本 / 类别标签） */
import type {
    AddedModSide,
    ModSearchPage,
    ModSearchQuery,
    ModSearchResult,
    ModSourceKind,
    ModTranslation,
    ModVersionEntry,
    SideFlag,
} from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 本地 jar 添加取证（Rust: inspect_added_mod(path)）：阶梯同整包分类，索引命中即零请求 */
export async function inspectAddedMod(path: string): Promise<AddedModSide> {
    if (!isTauri) return mock.mockInspectAdded(path);
    return invokeOrMock("inspect_added_mod", { path }, () =>
        mock.mockInspectAdded(path)
    );
}

/**
 * 在线添加那一行的端补查（Rust: inspect_added_build(sha1, fileName, title, cfClient?, cfServer?)）。
 * 构建给两样能当身份/证据的东西：字节 sha1（同一份 jar 两个平台逐字相同 → Modrinth 构建/项目层）
 * 与构建级端标签（CurseForge `gameVersions` 的 Client/Server，版本列表解析好的那对值原样传入，
 * CF 独占模组就靠它）。阶梯全空时回 `envSource: "unknown"`，调用方保持原样（别把已有的依据口径
 * 覆盖成没依据）
 */
export async function inspectAddedBuild(
    sha1: string,
    fileName: string,
    title?: string,
    cfClient?: SideFlag,
    cfServer?: SideFlag
): Promise<AddedModSide> {
    const offline = () => mock.mockInspectAddedBuild(sha1, fileName);
    if (!isTauri) return offline();
    return invokeOrMock(
        "inspect_added_build",
        {
            sha1,
            fileName,
            title: title ?? null,
            cfClient: cfClient ?? null,
            cfServer: cfServer ?? null,
        },
        offline
    );
}
export async function searchMods(query: ModSearchQuery): Promise<ModSearchPage> {
    if (!isTauri) return mock.mockSearch(query);
    return invokeOrMock("search_mods", { query }, () => mock.mockSearch(query));
}

/** 某模组的可用构建版本（Rust: list_mod_versions(source, modId)）：id 在两家不通用，必须带来源 */
export async function listModVersions(
    source: ModSourceKind,
    modId: string
): Promise<ModVersionEntry[]> {
    if (!isTauri) return mock.mockModVersions;
    return invokeOrMock(
        "list_mod_versions",
        { source, modId },
        () => mock.mockModVersions
    );
}

/**
 * 单个项目的展示信息（Rust: mod_detail(source, modId)）：详情页直跳一枚前置时，
 * 把版本列表给不了的那几样（简介/作者/图标/下载量/端标签）补齐。
 * 前置行先用手里已有的字段立即进入，这一发的结果到了再原地补全
 */
export async function modDetail(
    source: ModSourceKind,
    modId: string
): Promise<ModSearchResult> {
    if (!isTauri) return mock.mockModDetail(source, modId);
    return invokeOrMock("mod_detail", { source, modId }, () =>
        mock.mockModDetail(source, modId)
    );
}

/** 模组类别标签（Rust: list_mod_categories(source)）：两家的词表各自一套 */
export async function listModCategories(source: ModSourceKind): Promise<string[]> {
    if (!isTauri) return mock.mockModCategories;
    return invokeOrMock("list_mod_categories", { source }, () =>
        mock.mockModCategories
    );
}

/**
 * 详情页那枚「翻译」按钮要的中文译文（Rust: mod_translate_zh(source, slug, name?)）：第三方镜像的机翻件，
 * 名与简介一起拿，只在用户点击时发这一发，不进自动分类那条阶梯。
 * `name` 是详情页正在显示的模组名：CF 半边快照的 title 译文覆盖差（实测 jei/appleskin 空串），
 * 缺的那几格去镜像的 Modrinth 半边借，借之前拿它验同形（Rust: `translate_zh`）。
 * `null` = 镜像两份都还没译文（长尾收录里是空串）⇒ 调用方别切态；
 * 抛错只有网络故障一种。镜像只认 slug（CurseForge 的数字 id 实测查不到），
 * 所以没有 slug 时这枚钮压根不该出现
 */
export async function translateModZh(
    source: ModSourceKind,
    slug: string,
    name?: string
): Promise<ModTranslation | null> {
    const offline = () => mock.mockTranslateModZh(slug);
    if (!isTauri) return offline();
    return invokeOrMock(
        "mod_translate_zh",
        { source, slug, name: name ?? null },
        offline
    );
}


/** 在线添加与取证（本地 jar 取证 / 搜索 / 构建版本 / 类别标签） */
import type {
    AddedModSide,
    ModSearchPage,
    ModSearchQuery,
    ModSourceKind,
    ModTranslation,
    ModVersionEntry,
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
 * 在线添加那一行的端补查（Rust: inspect_added_build(sha1, fileName, title)）。
 * CurseForge 的响应里没有任何端声明，但它给了构建字节的 sha1 —— 同一份 jar 在两个平台
 * 哈希逐字相同，所以拿它走与本地 jar 同一条取证阶梯就能问到 Modrinth 的构建/项目层。
 * 阶梯全空时回 `envSource: "unknown"`，调用方保持原样（别把已有的依据口径覆盖成没依据）
 */
export async function inspectAddedBuild(
    sha1: string,
    fileName: string,
    title?: string
): Promise<AddedModSide> {
    const offline = () => mock.mockInspectAddedBuild(sha1, fileName);
    if (!isTauri) return offline();
    return invokeOrMock(
        "inspect_added_build",
        { sha1, fileName, title: title ?? null },
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

/** 模组类别标签（Rust: list_mod_categories(source)）：两家的词表各自一套 */
export async function listModCategories(source: ModSourceKind): Promise<string[]> {
    if (!isTauri) return mock.mockModCategories;
    return invokeOrMock("list_mod_categories", { source }, () =>
        mock.mockModCategories
    );
}

/**
 * 详情页那枚「翻译」按钮要的中文译文（Rust: mod_translate_zh(source, slug)）：第三方镜像的机翻件，
 * 名与简介一起拿，只在用户点击时发这一发，不进自动分类那条阶梯。
 * `null` = 镜像两份都还没译文（长尾收录里是空串）⇒ 调用方别切态；
 * 抛错只有网络故障一种。镜像只认 slug（CurseForge 的数字 id 实测查不到），
 * 所以没有 slug 时这枚钮压根不该出现
 */
export async function translateModZh(
    source: ModSourceKind,
    slug: string
): Promise<ModTranslation | null> {
    const offline = () => mock.mockTranslateModZh(slug);
    if (!isTauri) return offline();
    return invokeOrMock("mod_translate_zh", { source, slug }, offline);
}

/**
 * CurseForge Core API Key 的申请入口（第三方应用专用，免费，人工审核后把 Key 发到邮箱）。
 * 官方口径：console.curseforge.com 是「CurseForge for Studios」游戏方控制台，
 * 第三方模组服务走这张表单（docs.curseforge.com/rest-api 与帮助中心
 * 「About the CurseForge API and How to Apply for a Key」同一条链接）。
 * 设置页的「申请 Key」与「从网络添加模组」里的缺 Key 提示共用这一条地址。
 */
export const CURSEFORGE_APPLY_FORM =
    "https://forms.monday.com/forms/dce5ccb7afda9a1c21dab1a1aa1d84eb?r=use1";

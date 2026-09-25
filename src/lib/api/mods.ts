/** 在线添加与取证（本地 jar 取证 / 搜索 / 构建版本 / 类别标签） */
import type {
    AddedModSide,
    ModSearchPage,
    ModSearchQuery,
    ModSourceKind,
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

/** 搜索模组（Rust: search_mods(query) -> ModSearchPage） */
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

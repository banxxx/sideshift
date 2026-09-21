/** 解析与运行环境选项（包选择 / 解析 / 版本列表 / 默认选项 / 包内目录树） */
import type {
    ConversionOptions,
    PackDirNode,
    PackManifest,
    VersionOption,
} from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 打开系统文件选择器，返回选中的 .mrpack/.zip 路径（取消为 null）
 * 注：刻意用 plugin-dialog 模态框——曾试过无 owner 的 rfd 命令以支持"对话框开着
 * 也能拖文件进窗口"，但会引发弹窗失焦系列问题，已回退，勿再改回 */
export async function pickPackFile(): Promise<string | null> {
    if (!isTauri) return mock.mockManifest.fileName;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
        multiple: false,
        filters: [{ name: "整合包", extensions: ["mrpack", "zip"] }],
    });
    return typeof picked === "string" ? picked : null;
}

/** 打开系统文件选择器选择 .jar（Convert「从本地添加」；浏览器 dev 下返回 mock 文件名） */
export async function pickJarFile(): Promise<string | null> {
    if (!isTauri) return "krypton-0.2.3.jar";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
        multiple: false,
        filters: [{ name: "模组文件", extensions: ["jar"] }],
    });
    return typeof picked === "string" ? picked : null;
}

/** 解析整合包（Rust: parse_pack(path) -> PackManifest） */
export async function parsePack(path: string): Promise<PackManifest> {
    if (!isTauri) return mock.mockParsePack(path);
    return invokeOrMock("parse_pack", { path }, () => mock.mockParsePack(path));
}

/** MC 版本列表（Rust: list_mc_versions） */
export async function listMcVersions(): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockMcVersions;
    return invokeOrMock("list_mc_versions", undefined, () => mock.mockMcVersions);
}

/** 指定 MC 版本的加载器版本列表（Rust: list_loader_versions） */
export async function listLoaderVersions(
    mcVersion: string
): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockLoaderVersions;
    return invokeOrMock(
        "list_loader_versions",
        { mcVersion },
        () => mock.mockLoaderVersions
    );
}

/** 可用 Java 版本列表（Rust: list_java_versions） */
export async function listJavaVersions(): Promise<VersionOption[]> {
    if (!isTauri) return mock.mockJavaVersions;
    return invokeOrMock("list_java_versions", undefined, () => mock.mockJavaVersions);
}

/** 默认转换选项（Rust: default_options(manifest)） */
export async function defaultOptions(
    manifest: PackManifest
): Promise<ConversionOptions> {
    if (!isTauri) return mock.mockDefaultOptions;
    return invokeOrMock(
        "default_options",
        { manifest },
        () => mock.mockDefaultOptions
    );
}

/** 包内可保留目录树（Rust: list_pack_dirs） */
export async function listPackDirs(): Promise<PackDirNode[]> {
    if (!isTauri) return mock.mockPackDirs;
    return invokeOrMock("list_pack_dirs", undefined, () => mock.mockPackDirs);
}

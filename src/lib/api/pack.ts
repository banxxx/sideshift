/** 解析与运行环境选项（包选择 / 解析 / 版本列表 / 默认选项 / 包内目录树） */
import type {
    ConversionOptions,
    JavaProbe,
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

/** 让后端「最近一次解析的包」指向这个包（Rust: ensure_parsed(manifest) -> bool）。
 *  方案与目录树三个命令都只认那一个包，从任务里回看时必须先对准它。
 *  false = 内存没缓存且源文件已不在，只有需要重解析的明细（包内目录树）拿不到 */
export async function ensureParsed(manifest: PackManifest): Promise<boolean> {
    if (!isTauri) return true;
    return invokeOrMock("ensure_parsed", { manifest }, () => true);
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

/**
 * 本机 JDK 探测（Rust: probe_java）。开了「本机安装 Loader」才需要：那条路要就地跑 installer，
 * 缺 JDK 或版本不够属于**可预见的失败**，要在点转换之前看得见。
 * 回包里的 `installed` 就是转换页那颗下拉的候选 —— 事前检查与候选同源，不存在两份事实。
 * `requiredVersion` 传当前方案那档需求线（"17"）；`javaPath` 传用户手选的那枚（空=自动）。
 * 后端每次真跑一趟 `java -version`、不落缓存，所以装完 JDK 回到页面就该变绿、列表就该多出一枚。
 */
export async function probeJava(
    requiredVersion: string | null,
    javaPath?: string
): Promise<JavaProbe> {
    const path = javaPath?.trim() || null;
    if (!isTauri) return mock.mockJavaProbe(requiredVersion, path);
    return invokeOrMock("probe_java", { requiredVersion, javaPath: path }, () =>
        mock.mockJavaProbe(requiredVersion, path)
    );
}

/**
 * 某 MC 版本的 Java 需求线（Rust: `java_requirement`，表在 `core::java::required_for_mc`）。
 * 转换页换 MC 版本时要拿它改写方案里的 `javaVersion`：那张表只有后端一份，前端复刻一份
 * 就会跟实跑的那把筛子走偏。`javaVersion` 是快照字段（回看/重试读当时那一档），
 * 所以必须在方案落地时改写，而不是到了报告或实跑里再现算。
 */
export async function javaRequirement(mcVersion: string): Promise<string> {
    if (!isTauri) return mock.mockJavaForMc(mcVersion);
    return invokeOrMock("java_requirement", { mcVersion }, () =>
        mock.mockJavaForMc(mcVersion)
    );
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

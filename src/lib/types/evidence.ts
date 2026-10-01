/** 端判定证据的词表（Rust 侧同一套枚举，serde 字面量必须逐字对齐） */
import type { ModSourceKind } from "./mods";

/** 用户在添加那一刻钉住的构建（方案显示版本 = 构建下载版本） */
export interface PinnedVersion {
    /**
     * 构建直链。Modrinth 是 CDN 永久链；**CurseForge 恒为空串**——它的下载链是带时效的签名
     * URL，落档就会到期，所以构建时按 `source` + `fileId` 现取一条（Rust: `needs_curseforge_link`）。
     */
    url: string;
    sha1?: string;
    fileName: string;
    /** 来源平台；缺省 = Modrinth（老存档里只有这一家钉过） */
    source?: ModSourceKind;
    /** CurseForge 的 file id，与方案行 id（mod id）配对定位要取链的那份构建 */
    fileId?: string;
}

/** 端证据的来源（可信度由高到低；前端据此标注「依据什么判定」） */
export type EnvSource =
    /** mrpack files[].env —— 整合包作者显式声明 */
    | "mrpack"
    /** 包内 jar 的 fabric.mod.json environment —— 模组作者自证 */
    | "jarMetadata"
    /** Modrinth 按文件 sha1 反查构建 */
    | "modrinthHash"
    /** Modrinth 项目级 client_side/server_side */
    | "modrinthProject"
    /** 国内镜像（麦块开放 API）的项目级声明：内容同上一条，但是第三方快照、可能滞后 */
    | "mirrorProject"
    /** MC百科词条的「运行环境」：平台各腿全答不上时才问的补全源，社区编辑的第二手声明 */
    | "mcmod"
    /** 模组名关键字表，纯兜底 */
    | "nameHeuristic"
    /** 无任何证据：默认保留 */
    | "unknown";

/** 某一端的支持程度 */
export type SideFlag = "required" | "optional" | "unsupported";

/**
 * jar 字节码结构提示（不是证据层级，只影响「要不要靠文件名猜」）：
 * serverCode = jar 内确有服务端注册，名称关键字层被按住没删它；
 * clientOnlyShape = 只订阅客户端注册、形状像纯客户端（仅提示，处置不变）
 */
export type BytecodeHint = "serverCode" | "clientOnlyShape";

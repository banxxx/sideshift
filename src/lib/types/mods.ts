/** 版本下拉、在线搜索与本地添加取证 */
import type { LoaderKind } from "./pack";
import type { EnvSource, SideFlag, BytecodeHint } from "./evidence";

/** 版本下拉项（对应 Convert · Version Dropdown） */
export interface VersionOption {
    value: string;
    label: string;
    /** 推荐项（列表顶部高亮 + check） */
    recommended?: boolean;
    group?: string;
}

/** 在线搜索结果分页（对应 Online Add 分页页脚） */
export interface ModSearchPage {
    source: ModSourceKind;
    total: number;
    results: ModSearchResult[];
    /** 当前页（1 起） */
    page: number;
    pageSize: number;
}

/** 在线搜索的一个模组结果（对应 Online Add 行，模组级、无版本概念） */
export interface ModSearchResult {
    id: string;
    name: string;
    /** 一句话简介 */
    description: string;
    author: string;
    downloads: number;
    /** 图标 URL（可空则占位） */
    iconUrl?: string;
    source: ModSourceKind;
    /** 当前 MC/加载器下是否已有可用版本 */
    compatible: boolean;
    /** 是否已在新增列表中 */
    alreadyAdded: boolean;
    /** 项目级两侧支持度（Rust 由 Modrinth client_side/server_side 换算）：添加前端标签用；CurseForge 一律缺省 */
    clientSide?: SideFlag;
    serverSide?: SideFlag;
}

/** 搜索源：两家都真接；CurseForge 要用户自己的 API Key（Rust 缺 Key 时报可读错误） */
export type ModSourceKind = "modrinth" | "curseforge";

/** 在线搜索筛选参数 */
export interface ModSearchQuery {
    source: ModSourceKind;
    text: string;
    /** 空串 = 全部版本 */
    mcVersion: string;
    /** null = 任意加载器 */
    loader: LoaderKind | null;
    /** 类别过滤，可空=全部 */
    category?: string;
    page: number;
}

/** 某模组的一个可下载构建版本（对应 Mod Detail 版本行，整行点击下载） */
export interface ModVersionEntry {
    id: string;
    versionNumber: string;
    mcVersion: string;
    loader: LoaderKind;
    /** 发布时间 YYYY-MM-DD */
    date: string;
    sizeBytes: number;
    recommended: boolean;
    /** 该构建主文件直链（在线添加时随版本一起钉住）；CurseForge 给空串，见 `PinnedVersion.url` */
    url: string;
    sha1?: string;
    /** 服务端下载文件名 */
    fileName: string;
    /** 构建级 environment 换算出的两侧支持度：这一份构建要不要进服务端包；CurseForge 无此声明 → 两边都缺省 */
    clientSide?: SideFlag;
    serverSide?: SideFlag;
}

/** 「从本地添加」单个 jar 的取证返回（Rust inspect_added_mod） */
export interface AddedModSide {
    clientSide?: SideFlag;
    serverSide?: SideFlag;
    /** 结论出自哪一层；unknown = 三层都没答上（前端标需人工确认） */
    envSource: EnvSource;
    bytecodeHint?: BytecodeHint;
    /** 实际字节数（补上本地添加行原本缺失的体积） */
    sizeBytes?: number;
    /** jar 内自报 id / 显示名：文件名被改成中文时这才是可读名字 */
    modId?: string;
    title?: string;
}

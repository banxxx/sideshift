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
    /** 平台 URL slug：Modrinth 与 id 同值，CurseForge 另给（那边的 id 是数字）。
     *  「翻译」按钮要的那条 `detail/{slug}` 只认它，缺了就没有这枚钮 */
    slug?: string;
    name: string;
    /** 内置词典（MC百科词条名）按 slug 反查出的中文显示名，Rust 离线给出、零请求；
     *  没收录就缺省。只有简体中文档拿它盖过 `name`，`name` 始终是平台原名
     *  （方案与任务存档写的是那个原名） */
    nameZh?: string;
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

/** 详情页「翻译」那份中文译文（麦块镜像的机翻件）：名与简介各自可缺，
 * `title_zh` 覆盖率明显低于 `description_zh`（实测头部约 55% 对 100%） */
export interface ModTranslation {
    titleZh?: string;
    descriptionZh?: string;
}

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

/** 构建的一条前置（依赖）：id 是平台侧项目标识，name/slug 是后端批量反查的显示信息，查不到就缺省 */
export interface ModDependency {
    id: string;
    name?: string;
    /** 内置词典（MC百科词条名）按 slug 反查的中文显示名，与 `ModSearchResult.nameZh` 同口径 */
    nameZh?: string;
    slug?: string;
    /** required 必装 / optional 可选（incompatible / embedded 不会出现在表里） */
    required: boolean;
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
    /** 该构建声明的前置（两家平台的版本列表接口本就带着这份数据，名字由后端一次批量反查补上） */
    depends?: ModDependency[];
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

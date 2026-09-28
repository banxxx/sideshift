/** 整合包解析产物与包内结构 */

/** 模组加载器类型 */
export type LoaderKind = "fabric" | "forge" | "neoforge";

/** 解析出的整合包元信息（对应 Home 详情卡 + Convert 运行环境） */
export interface PackManifest {
    /** 原始文件名，如 vault-hunters-2.4.1.mrpack */
    fileName: string;
    /** 加载器 */
    loader: LoaderKind;
    /** Minecraft 版本，如 1.20.1 */
    mcVersion: string;
    /** 客户端模组总数 */
    modCount: number;
    /** 包体积（字节） */
    sizeBytes: number;
    /** 解析是否成功；失败时 error 描述原因 */
    parsed: boolean;
    error?: string;
    /** 源包绝对路径（后端下发、创建任务时原样带回） */
    sourcePath?: string;
}

/** 保留树里的一个文件条目；`keepFiles` 的取值 = 它的 `path`（逻辑相对路径，已小写） */
export interface PackFileNode {
    /** 文件名（不含路径），如 options.txt */
    name: string;
    /** 原始字节；0 = 未知（index 没给 fileSize 且 zip 条目也没测到） */
    sizeBytes: number;
    /** 从包根起算的逻辑相对路径（已剥 overrides 壳），如 kubejs/client_scripts/keep.js */
    path: string;
}

/** 包内可保留内容的整棵树（客户端保留内容弹窗数据源） */
export interface PackDirTree {
    /** 目录树（mods 与 resourcepacks 已在建树时跳过） */
    dirs: PackDirNode[];
    /** 根级散文件（`options.txt`、`servers.dat` 这类）：不带目录段，可以单独勾 */
    files: PackFileNode[];
}

/** 包内可保留目录树节点；keepDirs 条目 = 从包根起算的相对路径（如 kubejs/client_scripts） */
export interface PackDirNode {
    /** 目录名（不含路径），如 client_scripts */
    name: string;
    /** 该目录内文件数（递归，含子目录） */
    fileCount: number;
    /** 该目录内字节数（递归，含子目录）——勾选前得知道要带多大过去 */
    sizeBytes: number;
    /** **直属**文件（不含子目录里的），按名升序；只有展示，勾选仍走目录前缀 */
    files: PackFileNode[];
    /** 子目录节点，同层按名升序 */
    children: PackDirNode[];
}

/** 下载量预估（Rust: estimate_download，与构建 3.1–3.3 分类同源） */
export interface DownloadEstimate {
    /** 需联网下载的总字节（已剔除本地缓存命中与包内直取） */
    downloadBytes: number;
    /** 从源包/本地文件直取的总字节 */
    fromPackBytes: number;
    /** 是否全部有源可达；false = 有依赖拿不到大小或版本未解析，数字仅供参考 */
    complete: boolean;
}

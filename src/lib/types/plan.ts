/** 转换方案的一行模组与其处置结论（证据词表见 evidence.ts） */
import type { PinnedVersion, EnvSource, SideFlag, BytecodeHint } from "./evidence";

/** 单个模组在转换方案中的处置（对应 Convert 模组方案三行） */
export type ModDisposition = "remove" | "keep" | "add";

/** 转换方案里的一行模组 */
export interface PlanMod {
    id: string;
    name: string;
    /** 模组版本号 */
    version: string;
    /** 可选：兼容的加载器标注，如 Fabric */
    loader?: string;
    disposition: ModDisposition;
    /** 客户端专属（服务端需剔除） */
    clientOnly: boolean;
    /** 需人工确认（如 ViaFabricPlus） */
    needsReview: boolean;
    /** 自动补齐的依赖项（移除时给依赖警告） */
    autoSupplement: boolean;
    /** 源文件大小（字节）；0 = 未知（外部新增行构建期才解析） */
    sizeBytes?: number;
    /** 需联网下载（源声明带 URL）；false = 来自整合包或本地文件 */
    needsDownload?: boolean;
    /** 「从本地添加」的 .jar 绝对路径（真实后端直接取本地文件） */
    localPath?: string;
    /** 行 ↔ 整合包内条目的精确锚（Rust 下发，前端原样回传）：同 id 多文件时锁定正确条目 */
    srcPath?: string;
    /** 本行处置的证据来源；unknown = 没判定出来（默认保留） */
    envSource?: EnvSource;
    /** 更高可信层与整合包 files[].env 结论相反：处置按高可信层走，这里只作标注 */
    envConflict?: boolean;
    /** jar 字节码结构提示（不是证据层级，只影响「要不要靠文件名猜」） */
    bytecodeHint?: BytecodeHint;
    /** 客户端支持度；缺省 = 无证据 */
    clientSide?: SideFlag;
    /** 服务端支持度；与 clientSide 一起给出「客户端必需 / 服务端可选」这一直白证据 */
    serverSide?: SideFlag;
    /** 在线添加时钉住的构建；缺省 = 构建期解析最新兼容版（自动补行） */
    pinned?: PinnedVersion;
    /** 仅前端展示态：新增行被停用（行保留在清单、不参与构建与计数），下发前整行过滤 */
    disabled?: boolean;
    /** 依赖的其他方案行 id（mrpack depends 元数据；反向依赖警告用） */
    depends?: string[];
}

/** 自动分类结果（Rust emit("plan://classified", payload)）：在线层跑完后的增量刷新 */
export interface PlanClassified {
    /** 归属包名：前端按当前 manifest.fileName 校验，切包后的迟到事件一律丢弃 */
    fileName: string;
    /** 带全部证据层的完整方案（前端只套用用户未手动改过的行） */
    plan: PlanMod[];
    /** false = 本轮还没跑完（离线那次推送），true 才是最后一次事件 */
    done: boolean;
    /** 在线层是否全部成功（有请求失败 = false，前端提示可重试） */
    complete: boolean;
}

/** classify_pack 的同步返回：离线层结论 + 在线层还会不会再推一次事件 */
export interface PlanClassification {
    plan: PlanMod[];
    /** true = 联网反查已在后台起跑，最终结论走 plan://classified */
    onlinePending: boolean;
}

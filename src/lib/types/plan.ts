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
    /** CurseForge 编号行的取链探测结论：官方不放链、内容分发站也没有 ⇒ **构建期拿不到字节**。
     *  自动分类那一轮探的（一次空 JSON + 一次 HEAD，不下载内容），缺省 = 没探过（不是拿得到） */
    cfBlocked?: boolean;
    /** 整合包清单把这枚声明成 required（字段缺失按必需处理）：必选缺件拦「开始转换」，
     *  可选缺件跳过并逐条写进报告——少了可选模组不炸服，少了必选模组做出来的包跑不起来 */
    cfRequired?: boolean;
    /** 仅前端展示态：新增行被停用（行保留在清单、不参与构建与计数），下发前整行过滤 */
    disabled?: boolean;
    /** 依赖的其他方案行 id（mrpack depends + jar 自报硬依赖；反向依赖警告用） */
    depends?: string[];
    /** 把本行按住的依赖方行 id：本行被判客户端但被保留行（服务端必装的附属）硬依赖，
     *  依赖保护改判保留——非空时待人工理由要说清是谁按住的 */
    protectedBy?: string[];
    /** 剔除复核存疑：平台项目级判剔除、百科反驳但等级压不过（保留 + 待人工的依据） */
    doubted?: boolean;
}

/** 自动分类结果（Rust emit("plan://classified", payload)）：在线层跑完后的增量刷新 */
export interface PlanClassified {
    /** 归属包身份（绝对源路径）：前端按当前包校验，换包后的迟到事件一律丢弃。
     *  口径与 Rust `PackManifest::identity()` 一致 = `sourcePath ?? fileName` */
    packId: string;
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
    /** 这一包里按编号声明的 CF 行数（官方导出的 CF 包才有；带 jar 字节的民间包恒为 0）。
     *  包的属性、不是某一轮的补取结果 ⇒ 名字已被磁盘索引补过时它照旧是全部 */
    cfRows: number;
}

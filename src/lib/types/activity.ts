/** 取件构成与「当前动作」——实时条的数据源，只在进度事件上走，不进日志环 */

/**
 * 取件构成（Rust: 阶段 3 计划落定后写入）。只有 net* 走网络——
 * 界面据此区分「下载中」（真联网）与「取件中」（包内/本地/缓存，零流量）
 */
export interface FetchTally {
    /** 全部取件条目数 */
    files: number;
    /** 全部条目字节（大小未知条目计 0） */
    bytes: number;
    /** 需联网条目数 */
    netFiles: number;
    /** 需联网字节 */
    netBytes: number;
    /** 整合包内直取条目数 */
    packFiles: number;
    /** 本地文件复制条目数 */
    localFiles: number;
    /** 计划阶段就命中下载缓存的条目数 */
    cachedFiles: number;
}

/** 正在进行中的动作类型：联网收字节 / 本机跑 loader 安装器 / 本地写 zip */
export type ActivityKind = "net" | "install" | "zip";

/**
 * 当前动作。前端据此在日志区上方渲染一条实时条——
 * 逐块进度写成日志行会顶穿渲染预算
 */
export interface ActivityInfo {
    kind: ActivityKind;
    /** 联网 = 当前文件名；本机安装 = 「Forge 1.20.1-47.4.10」这样的一行主体；打包 = 当前顶层目录名（包根散件记「包根」） */
    subject: string;
    doneBytes: number;
    /** 0 = 总量未知（响应无 Content-Length；本机安装恒为 0，安装器不报总量） */
    totalBytes: number;
    itemsDone: number;
    /** 0 = 总量未知（本机安装同样恒为 0 ⇒ 实时条走不定态） */
    itemsTotal: number;
    /** 平均速率（字节/秒） */
    rateBps: number;
    /** 第几次尝试（1 起）；>1 = 前面失败过，要标出来 */
    attempt: number;
}

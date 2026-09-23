/** 应用设置（对应 Settings 屏 + 全局默认值） */

/** 下载源（Settings · 网络）：官方 / BMCLAPI 国内镜像，镜像不通自动回落官方 */
export type DownloadSource = "official" | "bmclapi";

export interface AppSettings {
    /** 服务端输出目录（报告页输出路径） */
    outputDir: string;
    /** 工作缓存目录：解析与构建的临时文件存放位置 */
    cacheDir: string;
    /** 剔除客户端专属资源（光影/小地图/键鼠等） */
    stripClientOnly: boolean;
    /** 构建后自检：打包完成时对产物离线对账（模组/jar/依赖/启动件/包根/保留目录），不起服务端进程 */
    verifyAfterBuild: boolean;
    /** 下载源：版本表与加载器 jar 的镜像档位（模组文件与端信息反查都在 Modrinth，无镜像） */
    downloadSource: DownloadSource;
    /** 并发下载数 1–16（同时决定反查端信息的并发请求数） */
    concurrency: number;
    /** 自动分类时允许联网反查 Modrinth（关掉了只剩包内自证 + 本地索引 + 名称兜底） */
    autoClassifyOnline: boolean;
}

/**
 * 缓存占用报表（Rust: cache_usage）。计数来自一次真实目录扫描，
 * 与「清理」用的是同一套分类判据——界面说的数和实际回收的量必须对得上。
 */
export interface CacheUsage {
    /** 缓存目录绝对路径，与设置里的 cacheDir 同源 */
    cacheDir: string;
    /** 目录不存在 = 还没下载过任何东西，此时所有计数为 0，不是错误 */
    exists: boolean;
    /** 可复用的下载缓存（跨任务共享，删了要重新联网下） */
    filesCount: number;
    filesBytes: number;
    /** 其中最后一次使用早于 staleDays 前的 */
    staleCount: number;
    staleBytes: number;
    /** 半截下载的临时文件；有任务在跑时不计（可能正被写着） */
    partsCount: number;
    partsBytes: number;
    /** 注册表里已无此任务的暂存目录：个数按目录算，不按里面的文件算 */
    orphanCount: number;
    orphanBytes: number;
    /** 空壳目录：零字节，但用户会当成「没清干净」，所以要外显也要能清 */
    emptyDirs: number;
    /** 有任务在排队或运行：「清空全部」禁用 */
    busy: boolean;
    /** 过期门槛（天）。后端常量，前端只读：显示的「30 天」与删除判据是同一个值 */
    staleDays: number;
}

/** 一次清理的真实结果：bytes 是删掉的每个文件删除前读到的字节合计，不是删除前的目录估算 */
export interface CleanReport {
    /** 删掉的条目数（一个孤儿暂存目录算一项，不摊成它内部的几百个文件） */
    items: number;
    bytes: number;
    /** 删不动的（被占用 / 权限）：非 0 时要外显，别让人以为清干净了 */
    failed: number;
}

/** 下载缓存的清理口径：只删过期 / 清空全部 */
export type CacheCleanMode = "stale" | "all";

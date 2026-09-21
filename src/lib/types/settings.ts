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

/** 应用设置（对应 Settings 屏 + 全局默认值） */

/** 下载源（Settings · 网络） */
export type DownloadSource = "official" | "bmclapi" | "github";

export interface AppSettings {
    /** 服务端输出目录（报告页输出路径） */
    outputDir: string;
    /** 工作缓存目录：解析与构建的临时文件存放位置 */
    cacheDir: string;
    /** 剔除客户端专属资源（光影/小地图/键鼠等） */
    stripClientOnly: boolean;
    /** 构建后自动校验：生成前空跑一次验证依赖完整性 */
    verifyAfterBuild: boolean;
    /** 下载源（Maven 镜像） */
    downloadSource: DownloadSource;
    /** 并发下载数 1–16 */
    concurrency: number;
    /** 自动分类时允许联网反查 Modrinth（关掉了只剩包内自证 + 本地索引 + 名称兜底） */
    autoClassifyOnline: boolean;
}

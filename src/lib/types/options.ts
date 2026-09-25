/** 转换可选项（对应 Convert 启动参数 + 运行环境 + 服务端设置） */
import type { CheckStatus } from "./report";

export interface ConversionOptions {
    /** 目标 Minecraft 版本 */
    mcVersion: string;
    /** 加载器版本，如 0.15.3 */
    loaderVersion: string;
    /** Java 版本，如 21 */
    javaVersion: string;
    /** 内存上限（MB） */
    memoryMb: number;
    /** 生成启动脚本 start.sh / start.bat */
    generateScripts: boolean;
    /** 无界面模式 --nogui */
    nogui: boolean;
    /** 自动写入 eula=true */
    agreeEula: boolean;
    /** 服务器端口（server.properties server-port） */
    serverPort: number;
    /** 服务器 MOTD */
    motd: string;
    /** 最大人数 */
    maxPlayers: number;
    /** 游戏模式 */
    gamemode: "survival" | "creative" | "adventure" | "spectator";
    /** 难度 */
    difficulty: "peaceful" | "easy" | "normal" | "hard";
    /** 正版验证 online-mode */
    onlineMode: boolean;
    /** 世界种子（空=随机） */
    levelSeed: string;
    /** Aikar's flags G1GC 调优参数组 */
    useAikarFlags: boolean;
    /** 附加 JVM 参数（原样拼入启动脚本） */
    extraJvmArgs: string;
    /** 本次输出目录覆写（空=用全局设置） */
    outputOverride: string;
    /** 原样带入服务端的包内目录：相对路径（任意层级），按前缀匹配拷贝 */
    keepDirs: string[];
    /**
     * 本次转换是否在本机跑 loader installer。初值取自全局设置，改过就存进这一包自己那份：
     * 重试与回看读的是快照，不跟随全局（同一份方案不该隔几天重跑做出不同的包）。
     */
    installLoaderLocally: boolean;
}

/**
 * 本机 JDK 探测结果（Rust: probe_java）。开关打开时转换页要在**点转换之前**就把
 * 「本机跑 installer 跑不跑得起来」说出来——可预见的失败不该排在几十秒下载后面才爆。
 * 状态口径沿用自检的三态，配色与图标不再分叉一份。
 */
export interface JavaProbe {
    status: CheckStatus;
    /** 选中那枚 java 的绝对路径；null = 本机压根没找到 */
    javaPath: string | null;
    /** 解析出的主版本（8/17/21/25…）；`java -version` 认不出格式时为 null */
    major: number | null;
    /** 本次转换的最低需求线；没传需求时 null = 只报有什么、不判够不够 */
    requiredMajor: number | null;
    /** 一句话结论（带真实数字与落点） */
    detail: string;
}

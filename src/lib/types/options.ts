/** 转换可选项（对应 Convert 启动参数 + 运行环境 + 服务端设置） */
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
}

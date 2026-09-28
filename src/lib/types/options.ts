/** 转换可选项（对应 Convert 启动参数 + 运行环境 + 服务端设置） */
import type { CheckStatus } from "./report";

export interface ConversionOptions {
    /** 目标 Minecraft 版本 */
    mcVersion: string;
    /** 加载器版本，如 0.15.3 */
    loaderVersion: string;
    /**
     * 本次的 Java **需求线**（如 "17"），由 MC 版本推出来，界面上不可编辑：它只当筛子
     * （够不够格、提示怎么说），不代表"要装哪版"，也不写进产物。
     */
    javaVersion: string;
    /**
     * 手选跑 installer 的那枚本机 JDK 绝对路径；空串 = 自动（需求线之上最低的那一枚，即推荐项）。
     * 与需求线分开存：换机或卸载后这条路径可能已经不存在，后端认不到就退回自动，不把转换钉死。
     */
    javaPath: string;
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
     * 原样带入服务端的包内**根级散文件**（如 `options.txt`）：按逻辑相对路径精确全等拷贝。
     * 与 `keepDirs` 分两个字段：目录走 `{path}/` 前缀、文件走全等，混成一个数组前端就得猜类型。
     * 根级文件没有父目录，所以这一档天然不会撞上「勾子项要不要收掉父级」那条互斥。
     */
    keepFiles: string[];
    /**
     * 本次转换是否在本机跑 loader installer。初值取自全局设置，改过就存进这一包自己那份：
     * 重试与回看读的是快照，不跟随全局（同一份方案不该隔几天重跑做出不同的包）。
     */
    installLoaderLocally: boolean;
    /**
     * 允许跳过「两条取链路都拿不到字节」的 CurseForge 模组继续构建。**缺省 = 关**（Rust 侧
     * `ConversionOptions` 整表 `#[serde(default)]`，旧档没这一键同样按不放行）：
     * 少一枚必选模组的包在服上多半起不来，那不是用户打算做的包 ⇒ 必须看过清单并显式同意。
     * 开了它 = 缺的那些逐条写进报告与包内 README，不是静默丢。
     */
    allowMissingMods?: boolean;
}

/**
 * 本机 JDK 探测结果（Rust: probe_java）。开关打开时转换页要在**点转换之前**就把
 * 「本机跑 installer 跑不跑得起来」说出来——可预见的失败不该排在几十秒下载后面才爆。
 * 状态口径沿用自检的三态，配色与图标不再分叉一份。
 */
export interface JavaProbe {
    status: CheckStatus;
    /** 本次**会用上**的那枚 java 的绝对路径（手选命中=手选那枚，否则=自动挑的）；null = 本机压根没找到 */
    javaPath: string | null;
    /** 解析出的主版本（8/17/21/25…）；`java -version` 认不出格式时为 null */
    major: number | null;
    /** 本次转换的最低需求线；没传需求时 null = 只报有什么、不判够不够 */
    requiredMajor: number | null;
    /** 本机扫到的全部 JDK（下拉候选，按 JAVA_HOME → PATH 顺序）；手选那枚也在其中 */
    installed: JavaInstall[];
    /** 传进去的「手选那枚」已经不在本机了（卸载/换盘符/换机），本次改用自动挑的那一枚 */
    selectedMissing: boolean;
    /** 一句话结论（带真实数字与落点），失败卡那一路用 */
    detail: string;
}

/** 本机一枚可用（`java -version` 认得出来）的 JDK：`path` 是标识，`major` 是显示名 */
export interface JavaInstall {
    path: string;
    major: number;
}

/**
 * 服务器端口的合法区间（TCP 全量可用端口；0 保留给「系统分配」，这里不给填）。
 * 单源：转换页那一格的红字与「开始转换」的禁用读的是同一个常量，
 * 分成两处写就会看到一个染红、另一个不拦。
 */
export const SERVER_PORT_RANGE = { min: 1, max: 65535 } as const;

/**
 * 会让 builder **让位**的两枚包根文件：文件名是 vanilla 写死的，用户在保留内容里勾了它就以包内那份为准，
 * 界面上对应的设置项这次不进产物（让位规则单源在后端 `src-tauri/src/core/builder.rs` 的 `emit_root`）。
 * 名字在这里单源：勾选弹窗、启动参数卡、服务端设置卡与报告都按它改口，否则界面在播报没进产物的配置。
 * `start.bat` / `start.sh` / `user_jvm_args.txt` 后端同样让位，但那一档没有成组的界面字段要灰，暂不挂进来。
 */
export const ROOT_EULA = "eula.txt";
export const ROOT_PROPERTIES = "server.properties";
export const YIELDED_ROOT_NAMES = [ROOT_EULA, ROOT_PROPERTIES];

/** 相对路径的落位名（最后一段）：勾选键恒小写，落位用的是条目自身名字，
 *  所以「这条勾上去在产物里叫什么」只能问它自己（后端单源：`parser::base_name`） */
export const baseName = (rel: string): string => rel.split("/").pop() ?? rel;

/**
 * 这枚包根文件在不在名单里（`keepFiles` / `reusedRootFiles` 都是包内相对路径）。
 * 比的是**落位名**而不是整条勾选键：勾深层那档（`Config/eula.txt`）落出来也是包根这枚，
 * 后端 `emit_root` 看的是磁盘上有没有它，前端要跟着同一件事改口才能对上
 */
export function holdsRootFile(names: string[] | undefined, name: string): boolean {
    return (names ?? []).some((n) => baseName(n).toLowerCase() === name.toLowerCase());
}

/** 名单里命中的让位文件名（勾选弹窗底栏用：只报真勾了的那几枚，不整句念规则） */
export function yieldedIn(names: string[]): string[] {
    return YIELDED_ROOT_NAMES.filter((n) => holdsRootFile(names, n));
}

/** 数字落在闭区间内（含端点）。非整数/NaN 一律算不合法 */
export const inRange = (v: number, r: { min: number; max: number }) =>
    Number.isFinite(v) && v >= r.min && v <= r.max;

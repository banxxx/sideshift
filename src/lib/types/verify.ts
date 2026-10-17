/**
 * 本机开服验证的载荷（Rust: `models/verify.rs`）：会话快照一次给全，进度事件只带增量。
 * 与 `types/report.ts` 的 `checks`（打包后离线对账、零子进程）分家：这一档起真进程、改的是解包副本。
 */
import type { BackendMsg } from "./l10n";

/** 会话状态（稿里的「通过带警告」不占一档：仍是 `passed`，只是 `warnings` 非空） */
export type VerifyStatus =
    | "ready"
    | "blocked"
    | "running"
    | "passed"
    | "failed"
    | "timeout"
    | "cancelled"
    | "missing";

/** 六步的 id：与 `core::verify_run` 的 `STEP_DEFS` 一一对应 */
export type VerifyStepId = "env" | "unpack" | "reconcile" | "loader" | "launch" | "wait";

export type StepStatus =
    | "pending"
    | "running"
    | "pass"
    | "warn"
    | "fail"
    /** 这一步在这一档根本不发生（Fabric 没有首启装加载器） */
    | "skipped";

/** 日志行的来源标签：界面按它着色 */
export type VerifySource =
    | "env"
    | "unpack"
    | "verify"
    | "loader"
    | "java"
    | "server"
    | "mixin";

export interface VerifyStep {
    id: VerifyStepId;
    /** 中文步骤名：前端按原文查表（与 `CheckResult::label` 同一口径） */
    label: string;
    status: StepStatus;
    /** 已走到的那几步的一句话摘要（带真实数字） */
    note: string;
    noteMsg?: BackendMsg | null;
}

export interface VerifyLogLine {
    /** HH:mm:ss */
    time: string;
    source: VerifySource;
    message: string;
    messageMsg?: BackendMsg | null;
    level: "info" | "warn" | "error";
}

/** 副本 `mods/` 里的一枚模组：`file` 是磁盘上的名字（停用那枚带 `.disabled`） */
export interface VerifyModEntry {
    file: string;
    name: string;
    enabled: boolean;
    sizeBytes: number;
}

/** 一句指向输出流的话：`line` 是本次输出流的序号，不是 `server.log` 的行号 */
export interface VerifyOutputRef {
    line: number;
    text: string;
}

/** 服务端自己喊出来的 WARN（原文，不归我们翻） */
export type VerifyWarning = VerifyOutputRef;

/** 失败现场：只摊证据，不给处置建议 */
export interface VerifyFailure {
    step: VerifyStepId;
    exitCode?: number | null;
    /** 报错行里点到的 jar 名（去重、截断） */
    jars?: string[];
    /** 最近的报错原文，时间顺序 */
    lines?: VerifyOutputRef[];
}

/** 一次验证的完整快照：页面一次拿全，进度事件只带增量行 */
export interface VerifySession {
    taskId: string;
    status: VerifyStatus;
    steps: VerifyStep[];
    logs: VerifyLogLine[];
    /** 副本根目录绝对路径；空串 = 还没解出来（或已经回收） */
    copyDir: string;
    copyBytes: number;
    modTotal: number;
    modDisabled: number;
    startedAt?: number | null;
    finishedAt?: number | null;
    elapsedSec: number;
    exitCode?: number | null;
    /** 副本 `server.properties` 里最终生效的那个端口 */
    serverPort?: number | null;
    /** 本次输出流读到的总行数（日志环只留最近 600 条，这个数说的是总量） */
    logLines: number;
    warnings?: VerifyWarning[];
    failure?: VerifyFailure | null;
    /** 不可判定的原因，一条一句 */
    blockers?: BackendMsg[];
    /** 失败定位卡的环境提示（目前只有 eula 未同意那一句） */
    hints?: BackendMsg[];
    /** 「验证参数」三项：脚本里的 `-Xmx…` 原样、`online-mode`、就绪上限秒数 */
    jvmXmx?: string | null;
    onlineMode?: boolean | null;
    timeoutSec: number;
    /** 构建时是否已把加载器并进产物（没有报告为 null）：决定「首启装加载器」那一格 */
    loaderInstalled?: boolean | null;
}

/** 模组启停的回执：会话里那几个计数跟着变，页面不必再拉整包 */
export interface ModToggleResult {
    entry: VerifyModEntry;
    mods: VerifyModEntry[];
    modTotal: number;
    modDisabled: number;
}

/** 进度事件载荷（Rust: `task_engine::verify::VerifyProgress`） */
export interface VerifyProgress {
    taskId: string;
    status: VerifyStatus;
    step?: VerifyStep | null;
    log?: VerifyLogLine | null;
}

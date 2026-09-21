/** 转换任务：状态机载荷与结构化错误 */
import type { PackManifest } from "./pack";
import type { ConversionOptions } from "./options";
import type { ActivityInfo, FetchTally } from "./activity";

/** 流水线四阶段（Shift Rail 站点，对应 Rust core 四模块） */
export type PipelineStage = "parser" | "detector" | "downloader" | "builder";

/** 任务状态 */
export type TaskStatus = "queued" | "running" | "success" | "failed" | "cancelled";

/** 一条流水线日志（对应控制台时间轴行） */
export interface TaskLogLine {
    /** HH:mm:ss */
    time: string;
    stage: PipelineStage;
    message: string;
    level: "info" | "warn" | "error";
}

/** 转换任务（对应 Task 页 / Tasks 列表卡 / Report） */
export interface ConversionTask {
    id: string;
    pack: PackManifest;
    options: ConversionOptions;
    status: TaskStatus;
    /** 当前阶段；未开始时为空 */
    stage?: PipelineStage;
    /** 总进度 0-100 */
    progress: number;
    /** 取件阶段计数（其余阶段可空）：downloaded/total 是全部条目，含包内与本地 */
    downloaded?: number;
    total?: number;
    /** 取件构成（阶段 3 计划落定后有值） */
    fetch?: FetchTally;
    /** 已完成的联网条目数（缓存命中不计） */
    netDone?: number;
    /** 已取回字节（含包内/本地/缓存） */
    doneBytes?: number;
    /** 当前动作（仅联网传输 / 打包进行中非空）：日志区上方实时条的数据源 */
    activity?: ActivityInfo;
    /** 创建/开始/结束时间（epoch ms） */
    createdAt: number;
    startedAt?: number;
    finishedAt?: number;
    /** 失败原因（status=failed 时有值） */
    error?: TaskError;
    /** 转换方案计数（任务信息卡「转换方案」行、列表卡副标题） */
    counts?: { remove: number; keep: number; add: number };
    /** 输出文件名 / 体积（成功时） */
    outputFileName?: string;
    /** 产物绝对路径：同名包加序号后与默认名不同名，重试时据此覆写自己那份 */
    outputPath?: string;
    outputSizeBytes?: number;
    logs: TaskLogLine[];
}

/** 创建/重试任务的返回（Rust: start_conversion / retry_task）；同一时间只跑一条，其余排队 */
export interface StartResult {
    taskId: string;
    /** true = 已有任务在跑，本次创建进入排队队列 */
    queued: boolean;
}

/** 结构化错误载荷（对应 Errors 族四张卡） */
export interface TaskError {
    /** 出错阶段 */
    stage: PipelineStage;
    /** 人类可读标题，如「依赖下载失败」 */
    title: string;
    /** 详情 */
    detail: string;
    /** 是否可重试 */
    retryable: boolean;
    /** 已重试次数（下载失败用） */
    attempts?: number;
    /** 关联日志尾（构建失败用） */
    logTail?: string[];
    /** 进程退出码（构建失败用） */
    exitCode?: number;
}

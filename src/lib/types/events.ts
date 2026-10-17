/** 后端事件契约（Rust app.emit ↔ 前端 listen） */
import type { ActivityInfo } from "./activity";
import type { PipelineStage, TaskLogLine } from "./task";

/** 流水线进度事件载荷（Rust app.emit("conversion://progress", payload)） */
export interface ProgressEvent {
    taskId: string;
    stage: PipelineStage;
    progress: number;
    downloaded?: number;
    total?: number;
    log?: TaskLogLine;
    /** 当前动作（联网传输 / 打包进行中）：前端直接刷新实时条，不必回拉任务 */
    activity?: ActivityInfo;
}

/** 后端事件名常量（与 Rust emit 字符串保持一致） */
export const EVENTS = {
    progress: "conversion://progress",
    done: "conversion://done",
    classified: "plan://classified",
    /** 应用更新取件进度（阶段 + 字节数同一个载荷，见 `types/update.ts` 的 `UpdateStatus`） */
    updateProgress: "update://progress",
    /** 启动后那一趟自动检查查出「有新版且能一键装」（载荷是 `UpdateInfo`）。只点角标，不弹窗 */
    updateAvailable: "update://available",
    /** 本机开服验证的一步换态 / 一行日志（载荷是 `VerifyProgress`，只带增量） */
    verifyProgress: "verify://progress",
    /** 一轮验证结束（通过/失败/超时/取消都发；载荷只有 taskId，页面据此回拉整份快照） */
    verifyDone: "verify://done",
} as const;

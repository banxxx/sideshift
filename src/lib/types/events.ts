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
} as const;

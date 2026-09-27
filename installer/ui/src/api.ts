/** 安装壳前端与壳后端（installer/src/main.rs）之间的契约，字段名与 Rust 侧 camelCase 序列化一一对应 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface DriveOffer {
    label: string;
    freeBytes: number;
    dataRoot: string;
}

export interface Plan {
    version: string;
    /** 流程条右侧那三个字母：x64 / arm64，由 Rust 按编译目标给，前端不猜 */
    arch: string;
    /** 够用的非系统固定盘，剩余空间降序；第一个就是预选档 */
    drives: DriveOffer[];
    /** 没有候选盘时的回落数据根（用户目录下） */
    fallbackDataRoot: string;
    installDir: string;
}

/** 用户挑的目录归一化后的结果。产物与缓存目录由路径规则从数据根推出，界面不逐条复述 */
export interface Layout {
    dataRoot: string;
}

/** 与 Rust `Stage` 的 kebab-case 对齐；新增阶段必须先证明"壳真的看得见它" */
export type Stage = "prepare" | "copy" | "data";

export interface Progress {
    stage: Stage;
    pct: number;
    done: boolean;
}

/** 安装结果：一律是 Rust 侧实际写盘/查到的路径，页面不再用自己那份状态拼展示 */
export interface Outcome {
    installedExe: string;
    dataRoot: string;
    /** 卸载入口没换成自带那套界面时的那句话（null = 已经指向它）。装是装成了，所以它不是错误 */
    uninstallNote: string | null;
}

export const getPlan = () => invoke<Plan>("get_plan");

export const resolveLayout = (root: string) =>
    invoke<Layout>("resolve_layout", { root });

export const runInstall = (dataRoot: string, installDir: string) =>
    invoke<Outcome>("run_install", { req: { dataRoot, installDir } });

export const cancelInstall = () => invoke<void>("cancel_install");

export const launchApp = (path: string) => invoke<void>("launch_app", { path });

/** 事件订阅：Rust 侧 emit 的是 installer://progress */
export const onProgress = (fn: (p: Progress) => void) =>
    listen<Progress>("installer://progress", (e) => fn(e.payload));

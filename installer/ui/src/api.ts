/** 安装壳前端与壳后端（installer/src/main.rs）之间的契约，字段名与 Rust 侧 camelCase 序列化一一对应 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface Plan {
    version: string;
    /** 标题栏右侧那三个字母：x64 / arm64，由 Rust 按编译目标给，前端不猜 */
    arch: string;
    /** 默认安装位置：与 NSIS 模板 currentUser 的默认值同源，用户不改就是它 */
    installDir: string;
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
    /** 程序实际落在哪（一屏版正文里那句「已装到」读它） */
    installDir: string;
    /** 卸载入口没换成自带那套界面时的那句话（null = 已经指向它）。装是装成了，所以它不是错误 */
    uninstallNote: string | null;
}

export const getPlan = () => invoke<Plan>("get_plan");

/** 数据跟着安装目录走（`appdata` / `cache` / `output` 三个平级目录，规则在 Rust 的 layout_in）：
 *  所以契约里只有一个目录，界面不再单独问"数据放哪" */
export const runInstall = (installDir: string) =>
    invoke<Outcome>("run_install", { req: { installDir } });

export const cancelInstall = () => invoke<void>("cancel_install");

export const launchApp = (path: string) => invoke<void>("launch_app", { path });

/** 事件订阅：Rust 侧 emit 的是 installer://progress */
export const onProgress = (fn: (p: Progress) => void) =>
    listen<Progress>("installer://progress", (e) => fn(e.payload));

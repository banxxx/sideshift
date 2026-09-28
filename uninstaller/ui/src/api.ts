/** 卸载壳前端与壳后端（uninstaller/src/main.rs）之间的契约，字段名与 Rust 侧 camelCase 一一对应 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** 开屏一次性的现场快照：`valid` 为假时界面上只有「关闭」可用（壳被单独拷出来跑就是这一态） */
export interface Snapshot {
    version: string;
    arch: string;
    valid: boolean;
    /** SideShift.exe 在不在跑。在跑就不让开始卸载，也不去强杀它 */
    running: boolean;
}

export interface Progress {
    pct: number;
    done: boolean;
}

/** 卸载完之后界面要复述的两条路径：Rust 侧卸载之后**实测还在**的那些，不是开屏那份快照。
 *  `cacheLeftover` 只在「该删而没删掉」时才有值（正常卸载它是 null） */
export interface Outcome {
    outputDir: string | null;
    cacheLeftover: string | null;
}

export const getSnapshot = () => invoke<Snapshot>("get_snapshot");

/** 轮询用：用户自己退出应用那一刻按钮就该放开，不该再多点一下 */
export const checkRunning = () => invoke<boolean>("check_running");

export const runUninstall = () => invoke<Outcome>("run_uninstall");

export const openPath = (path: string) => invoke<void>("open_path", { path });

/** 事件订阅：Rust 侧 emit 的是 uninstaller://progress */
export const onProgress = (fn: (p: Progress) => void) =>
    listen<Progress>("uninstaller://progress", (e) => fn(e.payload));

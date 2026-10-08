/** 卸载壳前端与壳后端（uninstaller/src/main.rs）之间的契约，字段名与 Rust 侧 camelCase 一一对应 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** 开屏一次性的现场快照：`valid` 为假时界面上只有「关闭」可用（壳被单独拷出来跑就是这一态） */
export interface Snapshot {
    version: string;
    arch: string;
    valid: boolean;
    /** SideShift.exe 在不在跑。在跑就点名它，但不去强杀它 */
    running: boolean;
    /** 转换产物现在躺在哪（没这个目录时 null，那句「留在…」不该指向空气） */
    outputDir: string | null;
    /** 那个目录是不是按规则建出来的那一个。人指过别处 ⇒ 那颗「同时删除」勾压根不出现 */
    outputMine: boolean;
}

export interface Progress {
    pct: number;
    done: boolean;
}

/** 卸载完之后界面要复述的路径：Rust 侧**卸载之后**实测还在的那些，不是开屏那份快照 */
export interface Outcome {
    outputDir: string | null;
    /** 这一趟是不是奉命删产物（勾了且归属判据认下）。配合 outputDir 决定那句是「留在…」还是「没能删掉」 */
    outputTargeted: boolean;
    /** 缓存目录还在：按清单收完但里面留着别人的东西，和整个目录本就不该动，两种都算 */
    cacheLeftover: string | null;
}

export const getSnapshot = () => invoke<Snapshot>("get_snapshot");

/** 轮询用：用户自己退出应用那一刻那行提示就该消失，不该再多点一下 */
export const checkRunning = () => invoke<boolean>("check_running");

export const runUninstall = (deleteOutput: boolean) =>
    invoke<Outcome>("run_uninstall", { deleteOutput });

/** 事件订阅：Rust 侧 emit 的是 uninstaller://progress */
export const onProgress = (fn: (p: Progress) => void) =>
    listen<Progress>("uninstaller://progress", (e) => fn(e.payload));

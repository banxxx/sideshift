/** 本机开服验证（会话快照 / 起停 / 副本模组 / 清理目录 / 两条事件） */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    ModToggleResult,
    VerifyModEntry,
    VerifyProgress,
    VerifySession,
} from "@/lib/types";
import { EVENTS } from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/** 会话快照（Rust: get_verify_state(id)）：六步 + 日志环 + 结论，一次拿全 */
export async function getVerifyState(id: string): Promise<VerifySession> {
    if (!isTauri) return mock.mockVerifyState(id);
    return invokeOrMock("get_verify_state", { id }, () => mock.mockVerifyState(id));
}

/** 开一轮验证（Rust: start_verify(id)）：同步返回只代表「排上了」，结果一律走事件与快照 */
export async function startVerify(id: string): Promise<void> {
    if (!isTauri) return mock.mockVerifyStart(id);
    return invokeOrMock("start_verify", { id }, () => mock.mockVerifyStart(id));
}

/** 取消（Rust: cancel_verify(id) -> bool）：false = 这一条现在根本没在跑 */
export async function cancelVerify(id: string): Promise<boolean> {
    if (!isTauri) return mock.mockVerifyCancel(id);
    return invokeOrMock("cancel_verify", { id }, () => mock.mockVerifyCancel(id));
}

/** 副本 mods 清单（Rust: list_verify_mods(id)）：启停弹窗两档共用一份表 */
export async function listVerifyMods(id: string): Promise<VerifyModEntry[]> {
    if (!isTauri) return mock.mockVerifyMods(id);
    return invokeOrMock("list_verify_mods", { id }, () => mock.mockVerifyMods(id));
}

/** 启停一枚模组（Rust: set_verify_mod_enabled(id, file, enabled)）：只改副本里的文件名 */
export async function setVerifyModEnabled(
    id: string,
    file: string,
    enabled: boolean
): Promise<ModToggleResult> {
    if (!isTauri) return mock.mockVerifySetMod(id, file, enabled);
    return invokeOrMock(
        "set_verify_mod_enabled",
        { id, file, enabled },
        () => mock.mockVerifySetMod(id, file, enabled)
    );
}

/** 清理验证目录（Rust: clean_verify_dir(id)）：副本整份删掉，会话回到默认那一档 */
export async function cleanVerifyDir(id: string): Promise<void> {
    if (!isTauri) return mock.mockVerifyClean(id);
    return invokeOrMock("clean_verify_dir", { id }, () => mock.mockVerifyClean(id));
}

/** 订阅验证进度（浏览器 mock 模式无事件流，返回空取消函数） */
export function onVerifyProgress(cb: (e: VerifyProgress) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<VerifyProgress>(EVENTS.verifyProgress, (ev) => cb(ev.payload));
}

/** 订阅一轮验证结束（浏览器 mock 模式无事件流，页面靠 mock 里的定时器轮询到终态） */
export function onVerifyDone(cb: (taskId: string) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<{ taskId: string }>(EVENTS.verifyDone, (ev) => cb(ev.payload.taskId));
}

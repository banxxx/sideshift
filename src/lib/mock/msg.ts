/**
 * mock 的一句话构造器：镜像 Rust 的 `msg!`——一条模板句同时给出「渲染好的中文整句」和「模板 + 参数」。
 * mock 里只给整句的话，界面上那条永远翻不出来、真机却翻得出来，自测会在两种数据下看到两套语言。
 */
import type { BackendMsg, TaskLogLine, VerifyStep } from "@/lib/types";

/** 日志行那一份：字段名跟着 `TaskLogLine` 走（`message` + `messageMsg`） */
export type LogSample = Pick<TaskLogLine, "message"> & Partial<Pick<TaskLogLine, "messageMsg">>;

/** 镜像 Rust 的 `l10n::value_text`：槽值贴进句子里的那段文本 */
function valueText(v: unknown): string {
    if (typeof v === "string") return v;
    if (v && typeof v === "object") {
        const m = v as Partial<BackendMsg>;
        // 槽值本身又是句模板时：内层连它一起渲染，不能 `[object Object]`
        if (typeof m.key === "string") return renderMsg(m.key, m.args ?? undefined).zh;
        return JSON.stringify(v);
    }
    return String(v);
}

export function renderMsg(key: string, args?: Record<string, unknown> | null) {
    const zh = key.replace(/\{\{\s*(\w+)\s*}}/g, (_, slot: string) => {
        const v = args?.[slot];
        // 漏传参数一眼看出来（Rust 同款：槽原样留着），不静默变空串
        return v == null ? `{{${slot}}}` : valueText(v);
    });
    return { key, args: args ?? null, zh };
}

/** 日志行 / 步骤摘要那一份：`message` + `messageMsg`，但字段名由调用点指定
 *  （任务日志与验证日志环用 `message`，六步的摘要是 `note` + `noteMsg`） */
export function sampleFor(
    msgField: "message" | "note",
    key: string,
    args?: Record<string, unknown>
): Record<string, unknown> {
    const m = renderMsg(key, args);
    return { [msgField]: m.zh, [`${msgField}Msg`]: m };
}

/** 日志行那一份（`message` + `messageMsg`） */
export function logMsg(key: string, args?: Record<string, unknown>): LogSample {
    return sampleFor("message", key, args) as LogSample;
}

/** 一步的摘要那一份（`note` + `noteMsg`） */
export function noteMsg(key: string, args?: Record<string, unknown>) {
    return sampleFor("note", key, args) as Pick<VerifyStep, "note" | "noteMsg">;
}

/** 一条只有模板、没有整句字段的 `Msg`（会话里的 blockers / hints 数组） */
export function backendMsg(key: string, args?: Record<string, unknown>): BackendMsg {
    return renderMsg(key, args);
}

/** 错误卡那一份：`detail` + `detailMsg` */
export function msg(key: string, args?: Record<string, unknown>) {
    const m = renderMsg(key, args);
    return { detail: m.zh, detailMsg: m };
}

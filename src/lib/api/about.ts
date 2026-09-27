/** 关于页 · 鸣谢名单（远端一份 JSON + 本机一份快照，逻辑全在 Rust: `core::ack`） */
import type { AckList } from "@/lib/types";
import * as mock from "@/lib/mock";
import { invokeOrMock, isTauri } from "./client";

/**
 * 读本机快照（Rust: ack_snapshot）。零网络、立刻返回；没有快照返回 null。
 * 进页第一下走它，界面上先有名单，再谈对账。
 */
export async function loadAckSnapshot(): Promise<AckList | null> {
    if (!isTauri) return mock.mockAckList;
    return invokeOrMock("ack_snapshot", undefined, () => mock.mockAckList);
}

/**
 * 拉一次远端名单（Rust: ack_refresh）：成功即由后端写回快照。
 * 失败一律 throw `net:` 码或 `ack:not-configured`——调用方（鸣谢名单那一块）不渲染这句话，
 * 只出「不可见 + 重新获取」：这一页不抢全局提示区，一次网络抖动也不该在关于页报错。
 * 纯浏览器 dev 下没有端点可打，mock 恒成功 ⇒ 空态要在壳内把 `ACK_ENDPOINT` 留空才看得到。
 */
export async function refreshAckList(): Promise<AckList> {
    if (!isTauri) return mock.mockAckList;
    return invokeOrMock("ack_refresh", undefined, () => mock.mockAckList);
}

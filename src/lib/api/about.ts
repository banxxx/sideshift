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

/**
 * 玩家名 → 正版皮肤贴图地址（Rust: `ack_skins`，链路与缓存见 `core::ack` 的「正版皮肤」一节）。
 *
 * 只有 `minecraftId` 为真的名字该送进来。查不到的人**不在返回的表里**，而不是给一个错误码：
 * 这一条链的失败形态就是「那张卡落回自带头像或首字」，关于页不为此报错。
 * 地址由后端给（Mojang 那两跳没有 CORS），**字节由 WebView 直连 Mojang 的贴图 CDN 取**
 * （那边带 `ACAO:*`，画布不脏）；后端只是把取到的字节存一份副本，见下面两条。
 */
export async function fetchAckSkins(names: string[]): Promise<Record<string, string>> {
    if (!isTauri) return mock.mockAckSkins;
    return invokeOrMock("ack_skins", { names }, () => mock.mockAckSkins);
}

/**
 * 读本机存的那份贴图字节，回 base64（Rust: `ack_skin_texture_get`）。
 * `null`＝没有副本（第一次见这张图、或上次没写成），调用方自己去打 CDN。
 * 键是地址末段那 64 位内容哈希 ⇒ 副本**不需要保鲜**：换皮肤必然换地址、必然换文件名。
 */
export async function loadTextureCache(url: string): Promise<string | null> {
    if (!isTauri) return null;
    return invokeOrMock<string | null>("ack_skin_texture_get", { url }, () => null);
}

/** 存一份贴图字节（Rust: `ack_skin_texture_put`）。发出去就不管结果：没落成只是下次再打一趟 CDN */
export function saveTextureCache(url: string, data: string): void {
    if (!isTauri) return;
    void invokeOrMock("ack_skin_texture_put", { url, data }, () => false).catch(() => {});
}

/**
 * 关于页「鸣谢名单」的线格式（Rust: `core::ack`，字段名与 serde camelCase 逐条对齐）。
 *
 * 这份 JSON 由远端给出、原样落本机快照，所以三件事写在 Rust 侧而不是这里：端点、
 * 头像主机的白名单、坏行的丢弃。前端拿到的东西已经过那三道，不必再判一遍。
 */

/** 一个人一张卡 */
export interface AckPerson {
    /** 卡上显示的名字；对 Minecraft 玩家它同时也是取皮肤用的玩家名（显示名与 ID 同源，不分两个字段） */
    name: string;
    /** 自带头像的 https 链接；取不到皮肤时回落它，两者都没有时落名字首字 */
    avatar?: string;
    /** 是不是 Minecraft 账号：真＝去取皮肤做头像，假＝直接用 `avatar` */
    minecraftId: boolean;
}

/** 一份名单 */
export interface AckList {
    /** 远端自己标的版本；与本机不同即覆盖，不做单调性判断 */
    version: string;
    people: AckPerson[];
}

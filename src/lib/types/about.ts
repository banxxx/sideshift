/**
 * 关于页「鸣谢名单」的线格式（Rust: `core::ack`，字段名与 serde camelCase 逐条对齐）。
 * 端点、头像主机白名单、坏行丢弃都在 Rust 侧做；前端拿到的已经过滤，不必再判。
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

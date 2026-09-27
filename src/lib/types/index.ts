/**
 * SideShift 领域类型 + IPC 契约（前端 ↔ Rust 的唯一数据接口定义）
 *
 * 约定：
 *  - 所有跨进程载荷集中在此目录，Rust 侧 serde 序列化的字段名必须与之对齐（camelCase）。
 *  - 这些类型对应 SS.pen 各屏所需数据；Rust command 以其为契约。
 *  - 金额/体积单位统一：字节用 number（bytes），时长用秒（number），进度用 0-100 整数。
 *
 * 按域分文件；调用方一律 `import type { … } from "@/lib/types"`，不必知道类型住在哪个文件。
 */
export * from "./pack";
export * from "./evidence";
export * from "./l10n";
export * from "./plan";
export * from "./options";
export * from "./activity";
export * from "./task";
export * from "./report";
export * from "./mods";
export * from "./settings";
export * from "./events";
export * from "./about";

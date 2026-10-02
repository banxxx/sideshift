/**
 * SideShift 领域类型 + IPC 契约（前端 ↔ Rust 唯一数据接口定义），按域分文件。
 * 硬约束：跨进程载荷字段名必须与 Rust serde 序列化的 camelCase 对齐；字节用 number、时长用秒、进度用 0-100 整数。
 * 调用方一律 `import type { … } from "@/lib/types"`，不必知道类型住在哪个文件。
 */
export * from "./pack";
export * from "./evidence";
export * from "./l10n";
export * from "./plan";
export * from "./options";
export * from "./template";
export * from "./activity";
export * from "./task";
export * from "./report";
export * from "./mods";
export * from "./settings";
export * from "./update";
export * from "./events";
export * from "./about";

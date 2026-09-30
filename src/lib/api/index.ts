/**
 * IPC 门面：前端唯一允许调用后端的地方，页面组件不许直接 invoke/listen。
 * 命令名、事件名、参数结构全部封装在本目录域文件（client/pack/plan/mods/task/settings/templates/system），Rust 落地只改这里。
 * 纯浏览器 dev 与 DEV 壳内命令缺失都回落 mock.ts，PROD 不受影响；真实分支的 invoke 字符串即 Rust 命令契约清单。
 */
export { isTauri } from "./client";
export * from "./pack";
export * from "./plan";
export * from "./mods";
export * from "./task";
export * from "./settings";
export * from "./templates";
export * from "./system";
export * from "./about";

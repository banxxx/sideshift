/**
 * IPC 门面：前端唯一允许调用后端的地方。
 *
 * 设计意图（对齐"公共抽取"要求）：
 *  - 页面组件只 import 本模块的函数，绝不直接 invoke/listen —— 后端命令名、事件名、
 *    参数结构全部封装在此，Rust 落地时只改本目录，页面零改动。
 *  - 运行环境自动探测：在 Tauri 内走真实 #[tauri::command]；纯浏览器 dev 下回落到
 *    mock.ts，让 UI 可脱离后端独立开发与演示。
 *  - DEV 下的 Tauri 壳内，若命令尚未落地（invoke 报 command not found），
 *    同样回落 mock，保证 `pnpm tauri dev` 也能走通全部 UI 流程；PROD 不受影响。
 *  - 每个真实分支的 invoke 字符串（如 "parse_pack"）即 Rust 命令契约清单。
 *
 * 分文件按业务域切：client（环境探测/回落入口）/ pack / plan / mods / task / settings / system。
 * 对外一律走本 barrel，import 路径仍是 "@/lib/api"。
 */
export { isTauri } from "./client";
export * from "./pack";
export * from "./plan";
export * from "./mods";
export * from "./task";
export * from "./settings";
export * from "./system";
export * from "./about";

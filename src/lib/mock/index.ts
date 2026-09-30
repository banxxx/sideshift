/**
 * Mock 数据源：后端 command 就绪前驱动全部 UI，仅供浏览器 dev；真实 command 落地后 api/ 门面自动切换。
 * 分文件：data（静态夹具）/ classify（分类与取证兜底）/ tasks（流水线定时器与历史样例）/ settings / templates。
 * 对外一律走本 barrel，import 路径仍是 "@/lib/mock"。
 */
export * from "./data";
export * from "./classify";
export * from "./tasks";
export * from "./settings";
export * from "./templates";

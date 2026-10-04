# 本地开发

## 环境要求

- Node.js 22 与 pnpm
- Rust stable（Windows 上为 MSVC 工具链）
- Windows 系统（应用当前只面向 Windows 打包）

## 常用命令

| 命令 | 作用 |
| --- | --- |
| `pnpm install` | 安装依赖 |
| `pnpm dev` | 仅启动前端（浏览器预览，后端功能为模拟数据） |
| `pnpm tauri dev` | 启动完整桌面应用（Rust 后端 + 前端） |
| `pnpm build` | 前端类型检查与构建 |
| `pnpm i18n:check` | 界面翻译覆盖率检查 |
| `pnpm docs:dev` | 本地预览文档站（本站） |
| `pnpm docs:build` | 构建文档站 |

Rust 测试在 `src-tauri` 目录下运行 `cargo test`。

## 项目结构

| 目录 | 内容 |
| --- | --- |
| `src/` | 前端（React + Vite）：界面、状态、与后端的 IPC 封装 |
| `src-tauri/` | 后端（Rust）：整合包解析、自动分类、下载、构建流水线、应用更新 |
| `installer/` | 安装壳（首次安装的引导程序） |
| `docs/` | 文档站（VitePress，即本站） |

## 提交与检查

推送到主分支会自动运行 CI（Rust 测试 + 前端构建 + 翻译检查）。
先在本地把这三样跑通再提交，可以省一轮等待。

## 相关页面

- [发版手册](/maintain/release)：打 tag 前后要做的事

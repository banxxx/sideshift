# 本地開發

## 環境需求

- Node.js 22 與 pnpm
- Rust stable（Windows 上為 MSVC 工具鏈）
- Windows 系統（應用目前只面向 Windows 打包）

## 常用命令

| 命令 | 作用 |
| --- | --- |
| `pnpm install` | 安裝依賴 |
| `pnpm dev` | 僅啟動前端（瀏覽器預覽，後端功能為模擬資料） |
| `pnpm tauri dev` | 啟動完整桌面應用（Rust 後端 + 前端） |
| `pnpm build` | 前端型別檢查與建置 |
| `pnpm i18n:check` | 介面翻譯覆蓋率檢查 |
| `pnpm docs:dev` | 本地預覽文件站（本站） |
| `pnpm docs:build` | 建置文件站 |

Rust 測試在 `src-tauri` 目錄下執行 `cargo test`。

## 專案結構

| 目錄 | 內容 |
| --- | --- |
| `src/` | 前端（React + Vite）：介面、狀態、與後端的 IPC 封裝 |
| `src-tauri/` | 後端（Rust）：整合包解析、自動分類、下載、建置流水線、應用程式更新 |
| `installer/` | 安裝殼（首次安裝的引導程式） |
| `docs/` | 文件站（VitePress，即本站） |

## 提交與檢查

推送到主分支會自動執行 CI（Rust 測試 + 前端建置 + 翻譯檢查）。
先在本地把這三樣跑通再提交，可以省一輪等待。

## 相關頁面

- [發版手冊](/zh-tw/maintain/release)：打 tag 前後要做的事

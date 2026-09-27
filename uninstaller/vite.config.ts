import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const here = import.meta.dirname;

/**
 * 卸载壳的前端。与设计体系的关系照抄安装壳：不复制、只引用。
 *  - `@` 指向主应用的 `src/`，按钮/标题栏几何/气泡与安装壳用的是同一批源码；
 *  - 令牌来自 `../../src/App.css`（见 ui/src/uninstaller.css 的 @import）；
 *  - 分叉的代价是「卸载器和安装器不是一套」，而这正是当初把壳画成这样的理由。
 */
export default defineConfig(() => ({
    plugins: [react(), tailwindcss()],
    root: path.resolve(here, "ui"),
    // root 在 uninstaller/ui，publicDir 默认指向不存在的 uninstaller/ui/public ⇒ /logo.svg 会 404。
    // 指回仓库根的 public/：图形与主应用同一份源文件，不复制第二份
    publicDir: path.resolve(here, "../public"),
    build: {
        outDir: path.resolve(here, "ui/dist"),
        emptyOutDir: true,
    },
    resolve: {
        alias: {
            "@": path.resolve(here, "../src"),
        },
    },
    clearScreen: false,
    server: {
        // 与主应用 1420、安装壳 1421/1422 错开：三套 dev server 可以同时开着对照
        port: 1423,
        strictPort: true,
        hmr: { protocol: "ws", host: "127.0.0.1", port: 1424 },
        watch: { ignored: ["**/src-tauri/**", "**/installer/target/**", "**/uninstaller/target/**"] },
    },
}));

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const here = import.meta.dirname;

/**
 * 安装壳的前端。刻意**不复制**设计体系：
 *  - `@` 指向主应用的 `src/`，按钮/卡片/气泡/格式化函数都是同一批源码；
 *  - 令牌来自 `../../../src/App.css`（见 ui/src/installer.css 的 @import）；
 *  - Tailwind 靠 @source 扫到那些共享组件里的 class，不手写第二套样式表。
 * 分叉的代价是「安装器长得像山寨版」，而合并的代价只是多打几 kB CSS。
 */
export default defineConfig(() => ({
    plugins: [react(), tailwindcss()],
    root: path.resolve(here, "ui"),
    // root 在 installer/ui，publicDir 默认就指向不存在的 installer/ui/public ⇒ /logo.svg 会 404。
    // 直接指回仓库根的 public/：图形与主应用同一份源文件，不复制第二份
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
        // 与主应用的 1420 错开：两套 dev server 可以同时开着对照
        port: 1421,
        strictPort: true,
        hmr: { protocol: "ws", host: "127.0.0.1", port: 1422 },
        watch: { ignored: ["**/src-tauri/**", "**/installer/target/**"] },
    },
}));

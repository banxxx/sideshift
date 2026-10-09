import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import process from "node:process";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync } from "node:fs";
import path from "node:path";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
    plugins: [react(), tailwindcss()],
    resolve: {
        alias: {
            "@": path.resolve(import.meta.dirname, "./src"),
        },
    },

  // 包版本号在构建期定死成常量（消费方：src/lib/api/settings.ts 的 APP_VERSION）。
  // 选 package.json 而非 tauri.conf.json：前端读得到、且它是 npm 生态的常规单源。
  define: {
    __APP_VERSION__: JSON.stringify(
      (
        JSON.parse(
          readFileSync(path.resolve(import.meta.dirname, "package.json"), "utf8")
        ) as { version: string }
      ).version
    ),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      // 监视成本按文件数付：dev 启动后第一个 CSS 请求要等这些目录逐个挂上监视。
      // 实测 66.4s → 17.6s（产物一字节不差），凶手就是 .scratch 的 6.2 万个文件与两处 cargo target。
      ignored: [
        "**/src-tauri/**",
        "**/.scratch/**",
        "**/installer/target/**",
        "**/uninstaller/target/**",
        "**/dist/**",
      ],
    },
  },
}));

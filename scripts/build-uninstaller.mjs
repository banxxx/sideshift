/**
 * 出卸载壳的发布产物（走 Tauri CLI，理由和安装壳一样：裸 cargo build --release 会带上 cfg(dev)，
 * 前端不内嵌，窗口开起来是「无法访问页面」——`uninstaller/src/main.rs` 里有 compile_error 拦着）
 *
 * 顺序上有硬性一条：卸载壳必须在**安装壳之前**出，因为安装壳的 build.rs 要把这份 exe 内嵌进去
 * （NSIS 只登记它自己的 uninstall.exe，卸载入口得靠安装那一刻投进去）。
 * 所以 `build-installer.mjs` 会先调本文件导出的 `buildUninstaller()`；单独跑它只为改样式时快一点。
 *
 * cargo 与主应用共用 src-tauri/target：三棵树依赖几乎重合，各留一份就是多占几 GB 磁盘。
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(import.meta.dirname, "..");
const releaseDir =
    process.env.SIDESHIFT_RELEASE_DIR ??
    path.join(root, "src-tauri", "target", "release");

export function buildUninstaller() {
    // tauri-build 要 <壳目录>/icons/icon.ico 才做得出 exe 图标；跟着主应用那份走，
    // 否则"pnpm tauri icon"重生成图标后，壳与应用的图标就悄悄分叉了。派生物、不入库
    const icon = path.join(root, "uninstaller", "icons", "icon.ico");
    mkdirSync(path.dirname(icon), { recursive: true });
    copyFileSync(path.join(root, "src-tauri", "icons", "icon.ico"), icon);

    console.log("▶ 用 Tauri CLI 编译卸载壳");
    // 直接跑 CLI 的 JS 入口而不是 `pnpm exec tauri`：execFileSync 不带 shell 时找不到 pnpm 的 .cmd 壳
    execFileSync(
        process.execPath,
        [
            path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js"),
            "build",
            "--no-bundle",
        ],
        {
            cwd: path.join(root, "uninstaller"),
            stdio: "inherit",
            env: {
                ...process.env,
                CARGO_TARGET_DIR: path.join(root, "src-tauri", "target"),
            },
        }
    );

    const out = path.join(releaseDir, "SideShift-Uninstall.exe");
    if (!existsSync(out)) {
        // CLI 报成功但产物不在 = 它按 cfg(dev) 那条路走了一半，或者 bin 名被改了。
        // 这里不硬拦的话，下一环安装壳会内嵌到一个不存在的文件上，报错信息还更难读
        console.error(`✗ 编译报告完成，但没看到 ${out}`);
        process.exit(1);
    }
    console.log(`\n产物：${out}`);
}

// 只有被直接执行时才跑（被 build-installer.mjs import 时由那边决定时机）。
// 走 realpath 比对：Windows 下 argv[1] 的盘符大小写与分隔符都不保证，字符串相等会漏
if (
    process.argv[1] &&
    realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
) {
    buildUninstaller();
}

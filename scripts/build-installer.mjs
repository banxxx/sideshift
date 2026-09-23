/**
 * 出安装壳的发布产物（走 Tauri CLI，它会自己跑 beforeBuildCommand 里的 vite build）
 *
 * 为什么是 CLI 而不是 cargo：裸 `cargo build --release` 会让 tauri-build 给壳打上 `cfg(dev)`，
 * 前端就不嵌进 exe，运行期去连 devUrl ⇒ 打开是「无法访问页面」。这条现在有 compile_error 拦着，
 * 见 `installer/src/main.rs`。
 *
 * 为什么要脚本：壳的 build.rs 要把主应用的 Setup.exe 打进二进制，顺序错了只会得到
 * "编译几分钟，最后一句找不到安装包"。这里先查一遍，把话说明白。
 *
 * cargo 与主应用共用 src-tauri/target：两边依赖几乎重合，各留一个 target 就是多占几 GB 磁盘。
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const releaseDir =
    process.env.SIDESHIFT_RELEASE_DIR ??
    path.join(root, "src-tauri", "target", "release");
const setupDir = path.join(releaseDir, "bundle", "nsis");

if (
    !existsSync(setupDir) ||
    !readdirSync(setupDir).some((f) => f.endsWith("-setup.exe"))
) {
    console.error(
        `找不到 NSIS 安装包（查过 ${setupDir}）。\n先出主应用的包：pnpm tauri build --bundles nsis`
    );
    process.exit(1);
}

// tauri-build 要 <壳目录>/icons/icon.ico 才做得出 exe 图标；跟着主应用那份走，
// 否则"pnpm tauri icon"重生成图标后，安装器与应用的图标就悄悄分叉了。
// 这份是派生物、不入库，所以新 clone 上目录本身也不存在 —— 先建目录再复制
const icon = path.join(root, "installer", "icons", "icon.ico");
mkdirSync(path.dirname(icon), { recursive: true });
copyFileSync(path.join(root, "src-tauri", "icons", "icon.ico"), icon);

console.log(`▶ 用 Tauri CLI 编译安装壳，内嵌 ${setupDir}`);
// 直接跑 CLI 的 JS 入口而不是 `pnpm exec tauri`：execFileSync 不带 shell 时找不到 pnpm 的 .cmd 壳
execFileSync(
    process.execPath,
    [
        path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js"),
        "build",
        "--no-bundle",
    ],
    {
        cwd: path.join(root, "installer"),
        stdio: "inherit",
        env: {
            ...process.env,
            SIDESHIFT_RELEASE_DIR: releaseDir,
            CARGO_TARGET_DIR: path.join(root, "src-tauri", "target"),
        },
    }
);

console.log(
    `\n产物：${path.join(releaseDir, "SideShift-Setup.exe")}`
);

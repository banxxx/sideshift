/**
 * 出便携版：SideShift/{SideShift.exe, portable.flag} 压成 zip
 *
 * 为什么不是 tauri build 顺手产：CLI 的 bundle 枚举里没有 portable（本地 config.schema.json 实测），
 * 便携版只能是手工产物 —— 所以它必须由脚本出，否则改了应用也不会重出，发出去的就是旧包。
 *
 * 为什么要自检 zip 条目：portable.flag 是应用判定数据根的唯一依据（src-tauri/src/core/data_root.rs），
 * 漏压它便携包会静默退化成安装版布局（数据写到 %APPDATA%），跑起来完全正常，没人会发现。
 */
import { execFileSync } from "node:child_process";
import {
    copyFileSync,
    existsSync,
    mkdirSync,
    readFileSync,
    rmSync,
    statSync,
    writeFileSync,
} from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const releaseDir =
    process.env.SIDESHIFT_RELEASE_DIR ??
    path.join(root, "src-tauri", "target", "release");

// 版本单源 = 根 package.json（与 vite define、两份 tauri.conf.json 同源）；Node 的 process.arch 已是 x64/arm64
const { version } = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
const name = `SideShift-${version}-portable-${process.arch}`;

const exe = path.join(releaseDir, "SideShift.exe");
if (!existsSync(exe)) {
    console.error(`找不到 ${exe}\n先出主应用：pnpm tauri build --bundles nsis`);
    process.exit(1);
}

const outDir = path.join(releaseDir, "bundle", "portable");
const innerDir = path.join(outDir, name, "SideShift");
const zip = path.join(outDir, `${name}.zip`);

// 路径一律走 env 传给 powershell：命令行里拼字符串会被空格和引号拆坏。
// OutputEncoding 不改的话 powershell 按本地代码页输出，中文全变问号。
const ps = (command, extraEnv = {}) =>
    execFileSync(
        "powershell",
        [
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            `[Console]::OutputEncoding=[Text.Encoding]::UTF8;${command}`,
        ],
        { encoding: "utf8", env: { ...process.env, ...extraEnv } }
    ).trim();

// exe 的版本资源来自编译期的 tauri.conf.json（它读的就是上面那个 package.json）。
// 对不上 = 这枚 exe 是改号之前编出来的，压进包就是"文件名 1.0.1、属性 1.0.0"的错包。
// 这道检查放在动手删东西之前：拦下来就不该顺手把上一次的产物毁掉。
const fileVersion = ps("(Get-Item $env:SS_EXE).VersionInfo.FileVersion", { SS_EXE: exe });
if (fileVersion !== version) {
    fail(
        `${exe} 的文件版本是 ${fileVersion || "(读不到)"}，与 package.json 的 ${version} 不一致。\n` +
            `  多半是改完版本号没重出主应用：pnpm tauri build --bundles nsis`
    );
}

rmSync(path.join(outDir, name), { recursive: true, force: true });
rmSync(zip, { force: true });
mkdirSync(innerDir, { recursive: true });
copyFileSync(exe, path.join(innerDir, "SideShift.exe"));
writeFileSync(path.join(innerDir, "portable.flag"), "");

ps("Compress-Archive -Path $env:SS_STAGE -DestinationPath $env:SS_ZIP -Force", {
    SS_STAGE: innerDir,
    SS_ZIP: zip,
});

const entries = ps(
    `[Reflection.Assembly]::LoadWithPartialName('System.IO.Compression.FileSystem')|Out-Null;` +
        `$z=[IO.Compression.ZipFile]::OpenRead($env:SS_ZIP);` +
        `$z.Entries|ForEach-Object{$_.FullName+' '+$_.Length};` +
        `$z.Dispose()`,
    { SS_ZIP: zip }
)
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
        const i = line.lastIndexOf(" ");
        return { name: line.slice(0, i).replace(/\\/g, "/"), size: Number(line.slice(i + 1)) };
    });
const listing = entries.map((e) => `${e.name} (${e.size})`).join("、") || "空";
const find = (n) => entries.find((e) => e.name === n);

const flag = find("SideShift/portable.flag");
const packed = find("SideShift/SideShift.exe");
if (!flag) fail(`压缩包里缺 portable.flag，实际条目：${listing}`);
if (!packed) fail(`压缩包里缺 SideShift.exe，实际条目：${listing}`);
if (packed.size !== statSync(exe).size) {
    fail(`包内 exe ${packed.size} 字节，与 ${exe} 的 ${statSync(exe).size} 不一致`);
}

console.log(`▶ ${zip}`);
console.log(`  ${statSync(zip).size.toLocaleString()} 字节，条目：${listing}`);

function fail(msg) {
    console.error(`便携版自检失败：${msg}`);
    process.exit(1);
}

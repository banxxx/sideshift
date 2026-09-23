# SideShift

Minecraft 模组包转换工具（Tauri 2 + React 19 + Vite）。开发环境细节见文末，本文重点是**出包与发版的注意事项**。

## 常用命令

| 命令 | 作用 |
| --- | --- |
| `pnpm dev` | 只起前端（vite，端口 1420） |
| `pnpm tauri dev` | 前端 + Rust 联调 |
| `pnpm build` | 前端类型检查 + 打包（`tsc && vite build`） |
| `pnpm tauri build --bundles nsis` | 出**安装版**（NSIS 安装包） |
| `pnpm installer` | 出**安装壳** `SideShift-Setup.exe`（内嵌上一步的 NSIS 包） |
| `pnpm portable` | 出**便携版** zip |
| `cargo test`（在 `src-tauri/`） | Rust 侧测试 |

## 三类产物与位置

| 产物 | 路径 | 说明 |
| --- | --- | --- |
| NSIS 安装包 | `src-tauri/target/release/bundle/nsis/SideShift_{版本}_x64-setup.exe` | 官方向导，负责注册卸载项、快捷方式、WebView2 引导 |
| 安装壳 | `src-tauri/target/release/SideShift-Setup.exe` | 自绘界面收集数据目录偏好，静默调上一步的 NSIS 包，**发出去的是这个** |
| 便携版 | `src-tauri/target/release/bundle/portable/SideShift-{版本}-portable-{arch}.zip` | 解压即用，数据跟着 exe 走 |

## 构建顺序不能反

1. `pnpm tauri build --bundles nsis`
2. `pnpm installer` —— **必须紧跟第 1 步**。壳的 `build.rs` 读 `release/SideShift.exe` 的当时大小做安装进度条的分母，而 Tauri 出包时会往这个 exe 里 patch bundle 信息（实测 9,999,360 → 10,688,000 字节）。中间插一次裸 `cargo build --release` 会把尺寸打回去，分母就偏小了。
3. `pnpm portable`

裸 `cargo build --release` 出不了壳的可用包：`tauri-build` 会给壳打上 `cfg(dev)`，前端不内嵌，打开是「无法访问页面」。这条已有 `compile_error!` 拦着（`installer/src/main.rs`），但**主应用的 exe 仍会被裸 cargo 覆盖**，所以顺序别乱。

安装壳与主应用共用 `src-tauri/target`（两边依赖几乎重合，各留一份要多占几 GB），因此两条 cargo 任务会互相等锁，只能串行。

## 版本号只有一个源

`package.json` 的 `version` 是真源，其余都是它的下游：

- `vite.config.ts` 在构建期读它并注入 `__APP_VERSION__` → 前端 `api.APP_VERSION`。
- 两份 `tauri.conf.json` 的 `version` 写成 `"../package.json"`。**这个相对路径是按 conf 文件所在目录解析的**（CLI 加载配置时 cwd 就是 `src-tauri/` / `installer/`），两边都正好指回根 `package.json`；写错会直接报 `failed to parse config: ... must be a semver string`，不会静默装错版本。
- 两份 `Cargo.toml` 手抄同一串，只为让 `CARGO_PKG_VERSION` 不说假话。

版本号语义：`主.次.修订[-预发布位]`，例 `1.0.0-beta.1`。带 `-beta.N` 的包在侧栏品牌行显示 BETA 徽章（`rc` 显示 RC，纯号不显示），左下角只显示短号 `v1.0.0`，完整版本串在设置页。

## 发版 checklist

- [ ] 改 `package.json` 的 `version`，并同步两份 `Cargo.toml`。
- [ ] 走上面三步出包，确认三个产物都还在（见「Defender」那条）。
- [ ] 仓库是 **public**。私有仓下匿名请求 `api.github.com/repos/banxxx/sideshift/releases` 回的是 404（本机实测：`git ls-remote` 有 master，但网页和 API 都 404 ⇒ 当前是私有），于是所有人的「检查更新」都会报 `Not Found`。
- [ ] GitHub Release 的 tag 用 `v{版本}`（`check_update` 会去掉开头的 `v` 再按 semver 解析）。
- [ ] **发 beta 必须勾 `pre-release`**。整套方案里只有这一步能真伤到用户：漏勾的那条 release 会被当成「最新版」，把测试包推给正式版用户。
- [ ] **重发同一个 beta 必须递增序号**（`beta.1` → `beta.2`）。已装旧版的人比出「相同」就永远收不到更新。
- [ ] 顺手记下安装包 SHA256，贴到分发页。

应用内「检查更新」读的是 `GET /repos/banxxx/sideshift/releases`（**不是** `/releases/latest`，那条官方定义就排除了 prerelease），按 `prerelease` 标志过滤、semver 比大小。用户在设置页「更新渠道」里手动选正式版 / Beta；没选过时跟随这枚包自己的版本号。

**预发布版本号在打包链上已实测通过**（`1.0.0-beta.1` 走完整 CLI 出包）：NSIS 产物名 `SideShift_1.0.0-beta.1_x64-setup.exe` 正常；主 exe / 壳 / 安装包 / 包内 exe 四处的 Win32 `FileVersion` 与 `ProductVersion` 都是 `1.0.0-beta.1`；壳的 `build.rs` 按字典序挑包这次选对了新版（见「安装壳挑包」那条）。**仍未实测**：真实安装与升级路径（要写用户机器，按惯例由开发者自测）、私有仓改成 public 之后 `check_update` 的真实返回。

## 已知坑

- **未签名**：NSIS 与便携 exe 都没有代码签名。表现是 SmartScreen 蓝底「未知发布者」，以及**杀软可能直接把产物清掉**（本机实测过 `bundle/` 下的包在二十分钟内凭空消失，没跑过任何删除命令）。出包后交给别人前重新确认文件在，必要机子上把 `src-tauri/target` 加进 Windows 安全中心排除项，并查「保护历史」。分发页要配一张「如何绕过 SmartScreen」的图。
- **便携版没有 WebView2 引导**：那段检测/下载在安装器模板里，不在 exe 内。目标机需自备 WebView2 运行时（Win10/11 一般已带）。
- **便携版靠 `portable.flag` 成立**：`SideShift/` 目录里缺这个标记文件，数据就写去 `%APPDATA%`，退化成安装版布局，而且运行起来完全正常、没人会发现。`scripts/build-portable.mjs` 压完会读一遍 zip 条目自检这一条。
- **便携版是手工产物，会过期**：改了应用不重跑 `pnpm portable`，发出去的就是旧 exe。脚本因此在打包前比对 `SideShift.exe` 的文件版本与 `package.json`，对不上直接报错退出（且不动上一次的产物）—— 看到这行就去重跑 `pnpm tauri build --bundles nsis`。
- **MSI 不做**：`bundle.targets` 已收窄到 `nsis`。MSI 只吃 `major.minor.patch` 三段号，带 `-beta` 会出问题，而且本机从未产出过 msi。
- **`productName` 曾是小写 `sideshift`**：改成 `SideShift` 后，老用户机器上「添加或删除程序」里会同时留着旧的 `sideshift` 卸载项（新卸载器只清自己的注册键），需提醒手动卸载一次。
- **元数据仍缺**：`publisher`（现在回落 identifier 第二段 `poso`）、`shortDescription` / `category` / `license` / `homepage` 都没配；`fileAssociations` 挂 `.mrpack` 也还没做。
- **安装壳挑包靠文件名字典序**（`installer/build.rs` 的 `find_setup`）：现在够用，但版本号进到会出两位小数的月份（例如 `0.9.0` 与 `0.10.0` 并存）时它会挑错，届时改成按 semver 或 mtime 选。
- **旧产物不会自己消失**：`bundle/` 下会同时留着 `SideShift_0.1.0_x64-setup.exe` 与 `SideShift-0.1.0-portable-x64.zip` 这类历史手工包，发版时按版本串核对文件名，别顺手拿错。

## 开发环境

推荐 VS Code + Tauri 扩展 + rust-analyzer。前端样式与组件令牌在 `src/App.css` 与 `src/components/ui/`，安装壳复用同一套（`installer/ui/`，别名 `@` 指向仓库根 `src/`）。窗口最小尺寸 1120×700 已锁。

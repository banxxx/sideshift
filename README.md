**简体中文** | [English](https://banxxx.github.io/sideshift/en/) | [繁體中文](https://banxxx.github.io/sideshift/zh-tw/)

<div align="center">

<img src="src-tauri/icons/128x128.png" alt="SideShift Logo" width="80" height="80">

# SideShift

![CI](https://img.shields.io/github/actions/workflow/status/banxxx/sideshift/ci.yml?style=for-the-badge&label=Build)
[![Issues](https://img.shields.io/github/issues/banxxx/sideshift?style=for-the-badge&label=issues&labelColor=444444&color=1F883D&logo=github)](https://github.com/banxxx/sideshift/issues)
![Downloads](https://img.shields.io/github/downloads/banxxx/sideshift/total?style=for-the-badge&label=downloads)
![License](https://img.shields.io/badge/license-Apache--2.0-blue?style=for-the-badge&logo=apache&label=license)

[下载](https://github.com/banxxx/sideshift/releases) |
[文档](https://banxxx.github.io/sideshift/)

[提交问题](https://github.com/banxxx/sideshift/issues/new) |
[本地开发](https://banxxx.github.io/sideshift/maintain/dev)

</div>

SideShift 把 Minecraft 模组整合包转换成**开箱即用的服务端包**：判断每个模组在服务端的去留、补齐前置、按需在本机安装加载器、生成启动脚本与常用服务端配置。

支持 `.mrpack`（Modrinth 整合包）、CurseForge 官方导出的 `.zip`，以及自带 `mods` 目录的普通 `.zip`。


## 💻 支持平台

| 操作系统 | 支持情况 | 环境要求 |
|---|---|---|
| Windows 10 / 11（x64） | ✅ 完整支持 | 无（WebView2 运行时系统一般已自带，安装版会引导） |
| Windows 7 / 8 / 8.1 | ❌ 不支持 | / |
| 便携版（`.zip`，任意 Windows 版本） | ⚠️ 需自备 WebView2 运行时 | WebView2 的检测与引导只在安装器里，不在 exe 内 |


**✅ 完整支持**：出问题按这条线排查，会给出完整答复。

**⚠️ 需自备运行时**：功能与安装版一致，只是首次打开可能提示缺 WebView2。

**❌ 不支持**：这些系统上打不开，请不要在此复现问题后反馈。

**注**：Beta 期间产物按版本递增发布，旧版本的问题请先在最新版本上复现。

## 🔒 许可证

本项目以 [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0) 发布，全文见仓库根目录的 [`LICENSE`](LICENSE)。

模组本身的版权归各原作者，本项目只是读取与搬运：转换结果里保留哪些模组由你决定，请自行遵守对应模组与平台的使用条款。

## 📚 数据来源与致谢

端信息、版本清单与文件字节取自下列公开服务：

[Modrinth](https://modrinth.com/) · [CurseForge](https://www.curseforge.com/) · [MC百科](https://www.mcmod.cn/)（内置中文名称词典）· [BMCLAPI](https://bmclapi2.bangbang93.com/) · [FabricMC](https://fabricmc.net/) · [Minecraft Forge](https://files.minecraftforge.net/) / [NeoForge](https://neoforged.net/) · Mojang 元数据

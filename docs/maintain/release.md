# 发版手册

应用内一键更新链的**发版半边**：签名出包 → 上传 Release。客户端那半边（检测、下载、验签、安装、对账）在 `src-tauri/src/core/update/`，两侧的契约是 release 里的那对资产：`*-setup.exe` + 同名 `.sig`。

## 一次性配置

### 1. 钥匙对

更新包的签名钥匙是 minisign 钥匙对。**公钥已经提交在仓库里**（两处，必须同串，有测试钉着）：

- `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`
- `src-tauri/src/core/update/mod.rs` → `UPDATER_PUBKEY`

如果你手上有与这把公钥配对的**私钥文件**（生成时的那把），直接进第 2 步；没有的话先生成一把新钥匙对，并按下面「换钥流程」把公钥同一次改进两处：

```bash
pnpm tauri signer generate -w .keys/sideshift.key
# 会打印公钥串，并写出私钥文件；密码可留空（留空时 Secrets 里的密码档也留空）
```

私钥文件**永远不进仓库**（`.keys/` 记得进 .gitignore）。丢了它 = 已发出的所有版本都没法再签，只能走换钥流程。

### 2. GitHub Secrets

仓库 Settings → Secrets and variables → Actions，配两条 Repository secret：

| 名字 | 值 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥**文件的内容**（整份粘贴；给路径官方明说不工作） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成时设的密码；没设密码就留一个空串 |

### 3. 验证整条链

不用真发一版：Actions 页手动跑一次 **Release** workflow（workflow_dispatch），它构建 + 签名但不建 release，产物留在本次运行的 artifacts 里。下载 `*-setup.exe` 与 `.sig`，装上后点「检查更新」，看到 `downloadable` 的结论即整条链已通（第一次配完后强烈建议走这一步）。

## 每次发版

1. **改版本号**：唯一真源是根 `package.json`（`tauri.conf.json` 用 `"../package.json"` 动态读它）；
   `src-tauri/Cargo.toml` 与 `installer/Cargo.toml` 跟着手动同步——只为让 `CARGO_PKG_VERSION` 不说假话，不改它编译能过但日志会说谎。
2. **版本号带不带预发布位就是渠道**：`1.0.0-beta.2` 归 Beta 线，`1.2.0` 归正式线（判据在 `release.rs::is_beta`，按 tag 的 semver 预发布位算，与 GitHub 的 prerelease 勾选无关）。
3. **写这一版的更新日志，两处各一份**：短的那份进弹窗，全的那份进文档站。

   **短日志 = `release-notes/<版本>.md`**（文件名不带 `v`：`release-notes/1.0.0-beta.3.md`）。它会被当成 GitHub Release 正文，也就是应用内「检查更新」弹窗里那几行。**一版一份、不覆盖旧的**——缺这个文件 workflow 直接 fail，所以「忘了换日志」没有发生条件。正文的规矩由 `node scripts/check-release-notes.mjs` 逐条判（打 tag 前自己跑一遍最省事）：

   | 规矩 | 为什么 |
   |---|---|
   | 最多 4 行，推荐写 3 行 | 弹窗的说明是 `line-clamp-4`，第 5 行永远看不到；留一行是给字号/DPI 变化兜底 |
   | 一行一条，说不完就拆两条 | 一个硬换行就是一个视觉行；单行约 34 个汉字（418px 内容宽 ÷ 12px 字号），写满会绕成两行、把下一行的预算吃掉 |
   | 条首统一 `- ` | 发布页会渲染成列表，弹窗里读作项目符；除此之外别用 markdown |
   | 不写空行 | 弹窗按 `whitespace-pre-wrap` 渲染，空行会显示成一行 18px 的空白 |
   | 不写标题 / `<!-- 注释 -->` / `---` 头 | 弹窗不渲染 markdown，这些符号会原样出现在用户眼前 |
   | 不写版本号、日期、包体大小 | 标题已经是「更新到 vxxx」，标题下那行小字已经有 大小 · 日期 · 当前版本，重复就是白占 |
   | 顺序：新增 → 改动 → 修正 | 用户先要判断这版有没有自己等的东西 |

   **全日志 = `docs/guide/changelog.md`**（弹窗左下角那颗「更新记录」跳的就是它）：在**最上面追加**一节，旧的往下堆，不删不改。那一节的标题**只能写纯版本号**（`## v1.0.0-beta.3`），日期写进正文第一行——那颗钮是按版本号算锚点跳的（`v1.0.0-beta.3` → `#v1-0-0-beta-3`），标题里多一个符号锚点就对不上了。

   **顺序有硬要求**：这两处改动先合进 master、等 Pages 把文档站构建发布完，**再**打 tag。因为 Pages 吃 push→master、Release 吃 tag push，反过来的话用户点进去看到的是一页还没有这一节的记录。


4. **打 tag 并推送**（tag 必须以 `v` 开头，且 `v` 后面与 package.json 逐字一致——不一致 workflow 会直接 fail）：

   ```bash
   git tag v1.0.0-beta.3
   git push origin v1.0.0-beta.3
   ```

5. workflow 自动：核对这一版的 `release-notes/<版本>.md` 存在且形状合格 → 构建 NSIS + 签名 → 出安装壳与便携包 → 建 Release（正文取那份文件，预发布位自动判断）→ 上传四个产物。


## 版本线：正式版与测试版

两条线互相独立，tag 的预发布位决定一切：

| 你打的 tag | GitHub 上的标记 | 谁会收到 |
| --- | --- | --- |
| `v1.0.0-beta.N` | 自动标 Prerelease | 只有「Beta 线」用户 |
| `v1.1.0` | 正式 Release | 只有「正式线」用户 |

Beta 线用户**收不到正式版推送**：装着 `beta.4` 的用户在 `1.1.0` 发布后不会收到提示——
他的渠道里只有 beta 系。发正式版时记得另行公告，或在设置里引导切换渠道。

## Release 上的四个产物

| 文件 | 谁用它 |
|---|---|
| `SideShift_<版本>_x64-setup.exe` | **应用内更新的主包**（`UpdateAssetKind::Package`，签名覆盖的是它） |
| `SideShift_<版本>_x64-setup.exe.sig` | 与上面同名配对的签名，缺它整条降级（`NoSignature`） |
| `SideShift-Setup.exe` | 安装壳：发布页上给人**首次安装**用的那份（内嵌主安装包与卸载壳） |
| `SideShift-<版本>-portable-x64.zip` | 便携版（带 `portable.flag`；便携形态下应用内更新自动降级为发布页） |

注意 `SideShift-Setup.exe`（首装壳）**不参与**应用内更新——更新器装的是签名覆盖的那对。

## 换钥流程

换公钥必须与构建脚本同一次改：`tauri.conf.json` 的 pubkey 与 `core/update/mod.rs` 的 `UPDATER_PUBKEY` 是两处字面量，有测试钉着同串，但只改一侧产出的包会「签得上、老客户端读不懂」。所以换钥要**连着发一版只为改钥的包**：新钥签出的版本，老钥客户端验不过，只能走「打开发布页」手动装一次；装上新钥构建之后，链路恢复。

## 排查

| 症状 | 病因 |
|---|---|
| workflow 在「核对 tag」一步挂了 | tag 与 package.json 版本不一致（版本号唯一真源是 package.json） |
| workflow 在「核对更新日志已填写」一步挂了 | 缺 `release-notes/<版本>.md`（一版一份，文件名不带 `v`），或它的形状不合格：有空行、有 markdown、合计超过 4 个视觉行——本地跑 `node scripts/check-release-notes.mjs` 就能看到是哪一行 |
| 弹窗里的日志被截在一半 | 那份日志的视觉行数顶到了 4 行上限、且最后一条绕了行；把长的那条拆短，完整改动写进 `docs/guide/changelog.md` |

| 构建报「A public key has been found, but no private key」 | Secrets 没配或没配对：`TAURI_SIGNING_PRIVATE_KEY` 必须是钥匙文件的内容 |
| 用户端停在「缺同名签名文件」 | release 里 `.sig` 没传上（检查 workflow 的产物收集步骤是否跑全） |
| 用户端停在「签名与安装包对不上」 | 公钥换过但客户端没换：走换钥流程 |
| 「检查更新」失败 | API JSON 无公共镜像（ghproxy 系实测拒代理 `api.github.com`），走官方直连（10s 短超时 × 2 次尝试）；**下载**有镜像候选链（ghfast.top / ghproxy.net，官方殿后），镜像烂了从 `core/update/mod.rs` 的 `ASSET_MIRRORS` 摘掉即可 |

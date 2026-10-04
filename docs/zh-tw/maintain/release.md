# 發版手冊

應用程式內更新鏈的**發版半邊**：簽章出包 → 上傳到 GitHub Release。
用戶端那半邊（檢測、下載、驗簽、安裝、對帳）在 `src-tauri/src/core/update/`，
兩側的契約是 release 上的那對資產：`*-setup.exe` 加同名 `.sig`。

## 一次性設定

### 1. 金鑰對

更新包的簽章金鑰是 minisign 金鑰對。**公鑰已經提交在倉庫裡**（兩處，必須同串，有測試釘著）：

- `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`
- `src-tauri/src/core/update/mod.rs` → `UPDATER_PUBKEY`

如果你手上有與這把公鑰配對的**私鑰檔案**，直接進第 2 步；沒有的話先生成一把新金鑰對，
並按下面「換鑰流程」把公鑰同一次改進兩處：

```bash
pnpm tauri signer generate -w .keys/sideshift.key
```

私鑰檔案**永遠不進倉庫**（`.keys/` 記得進 .gitignore）。丟了它 = 已發出的所有版本都沒法再簽，只能走換鑰流程。

### 2. GitHub Secrets

倉庫 Settings → Secrets and variables → Actions，配兩條 Repository secret：

| 名稱 | 值 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 私鑰**檔案的內容**（整份貼上；給路徑官方明說不工作） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成時設的密碼；沒設密碼就留一個空串 |

### 3. 驗證整條鏈

不用真發一版：Actions 頁手動跑一次 **Release** workflow（workflow_dispatch），
它建置 + 簽章但不建 release，產物留在本次運行的 artifacts 裡。
下載 `*-setup.exe` 與 `.sig`，裝上後點「檢查更新」，看到可一鍵安裝的結論即整條鏈已通。

## 每次發版

1. **改版本號**：唯一真源是根 `package.json`（`tauri.conf.json` 用 `"../package.json"` 動態讀它）；
   `src-tauri/Cargo.toml` 與 `installer/Cargo.toml` 跟著手動同步——只為讓 `CARGO_PKG_VERSION` 不說假話。
2. **版本號帶不帶預發布位就是頻道**：`1.0.0-beta.2` 歸 Beta 線，`1.2.0` 歸正式線
   （判據在 tag 的 semver 預發布位，與 GitHub 的 prerelease 勾選無關）。
3. **打 tag 並推送**（tag 必須以 `v` 開頭，且 `v` 後面與 package.json 逐字一致——不一致 workflow 會直接失敗）：

   ```bash
   git tag v1.0.0-beta.3
   git push origin v1.0.0-beta.3
   ```

4. workflow 自動：建置 NSIS + 簽章 → 出安裝殼與可攜包 → 建 Release（預發布標記自動判斷）→ 上傳四個產物。

## Release 上的四個產物

| 檔案 | 誰用它 |
| --- | --- |
| `SideShift_<版本>_x64-setup.exe` | **應用程式內更新的主包**（簽章覆蓋的是它） |
| `SideShift_<版本>_x64-setup.exe.sig` | 與上面同名配對的簽章，缺它整條降級（`NoSignature`） |
| `SideShift-Setup.exe` | 安裝殼：發布頁上給人**首次安裝**用的那份（內嵌主安裝包與卸載殼） |
| `SideShift-<版本>-portable-x64.zip` | 可攜版（帶 `portable.flag`；可攜形態下應用程式內更新自動降級為發布頁） |

注意 `SideShift-Setup.exe`（安裝殼）**不參與**應用程式內更新——更新器裝的是簽章覆蓋的那對。

## 換鑰流程

換公鑰必須與建置腳本同一次改：`tauri.conf.json` 的 pubkey 與 `core/update/mod.rs` 的 `UPDATER_PUBKEY`
是兩處字面量，有測試釘著同串，但只改一側產出的包會「簽得上、老用戶端讀不懂」。
所以換鑰要**連著發一版只為改鑰的包**：新鑰簽出的版本，老鑰用戶端驗不過，只能走「打開發布頁」手動裝一次；
裝上新鑰建置之後，鏈路恢復。

## 疑難排解

| 症狀 | 病因 |
| --- | --- |
| workflow 在「核對 tag」一步掛了 | tag 與 package.json 版本不一致（版本號唯一真源是 package.json） |
| 建置報「A public key has been found, but no private key」 | Secrets 沒配或沒配對：`TAURI_SIGNING_PRIVATE_KEY` 必須是鑰匙檔案的內容 |
| 用戶端停在「缺同名簽章檔案」 | release 裡 `.sig` 沒傳上（檢查 workflow 的產物收集步驟是否跑全） |
| 用戶端停在「簽章與安裝包對不上」 | 公鑰換過但用戶端沒換：走換鑰流程 |
| 「檢查更新」一直失敗 | API JSON 無公共鏡像（ghproxy 系實測拒代理 `api.github.com`），走官方直連（10 秒短逾時 × 2 次嘗試）；**下載**有鏡像候選鏈（ghfast.top / ghproxy.net，官方殿後），鏡像爛了從 `core/update/mod.rs` 的 `ASSET_MIRRORS` 摘掉即可 |

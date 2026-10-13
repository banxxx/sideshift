# 关于页「鸣谢名单」的 Cloudflare Worker

一个 Worker + 一个 KV：发一份名单 JSON，顺带缓存 Minecraft 玩家名 → 贴图地址。

三条前提，代码只按这三条写：**只有 `/submit` 一个写入口**（带口令才算写，名单原样透传、一个字段都不重新序列化）；**贴图字节不过这里**（`/skins` 只回 `textures.minecraft.net` 的 URL，字节由客户端直连 Mojang CDN）；**Mojang 的错误绝不落表**（404 / 429 / 5xx / 空 body / 非法 body 一律不写 KV——否则"名字暂时查不到"会被冻成"永久返回一个错误的 UUID"，那比查不到难修一个数量级）。

命令默认在 **Windows PowerShell 5.1**（Win10 自带那个蓝底窗口）里跑，每段另给 bash 形态。
抄任何一条命令前，先扫文末「PowerShell 5.1 的口径」——那五条规矩是绕不开的，不是风格偏好。

---

## 先认清五份"同名不同物"的东西

这条链上 `contributors.json` 这个名字出现在**三个互不相干的地方**，另有两份只在仓库/开发机上的近亲。第一次碰这里的人十有八九会搞混，所以先摆一张表：

| 叫什么 | 在哪 | 谁往里写 | 谁读它 | 进 git |
| --- | --- | --- | --- | --- |
| `contributors.sample.json` | 仓库 `deploy/ack/` | 只有你**改字段形状**时才动 | 谁都不读，它是给人抄形状的模板（带三条示例行，含 `{ENDPOINT_HOST}` 的写法示范） | **是** |
| `contributors.local.json` | 你的开发机 | 你手写，或从线上 GET 下来 | 只有你那条 `curl --data-binary "@…"` 读它 | 否（`.gitignore`） |
| `contributors.prev.json` | 你的开发机 | `Copy-Item` 留的上一版 | 只有你要回滚时读 | 否（`.gitignore`） |
| 键 `contributors.json` | **Cloudflare KV** | Worker 的 `POST /submit`（`env.ACK.put`） | Worker 的 `GET /contributors.json`（每次请求现读 `env.ACK.get`，不常驻内存） | 不在仓库 |
| `contributors.json`（同名！） | **用户机器的配置目录** | 客户端 `core/ack.rs` 的 `write_snapshot` | 每次进关于页先读它（离线也上屏）；它不在任何"可清理"类目里 | 不在仓库 |

由此得出两条常被问住的话：

- **本地那份文件叫什么，推上去之后就丢了。** `--data-binary "@<路径>"` 只把文件的**字节**当请求体，Worker 把它 `put` 到自己那个固定键名（`worker.js` 顶部的 `LIST_KEY = "contributors.json"`）。所以"要推哪个文件"没有唯一答案——要问的是**哪份文件的内容是你想让线上变成的那一份**。
- **控制台 KV 的 Explorer 是"打开那一页时"的快照**，不跟着 `put` 刷新。线上到底是不是新值，别拿面板当判据：`curl.exe -sS "$AckUrl/submit?key=$AckKey"` 那一条才是直读 KV（它显式 `no-store`），硬刷新面板（`Ctrl+F5`）只是让显示层追上。

---

## 占位符（本文档没有一处真实信息）

| 写法 | 是什么 | 真实值在哪 |
| --- | --- | --- |
| `ack.example.com` | 你的自定义域（占位） | Cloudflare 已绑的 route ＋ `src-tauri/src/core/ack.rs` 的 `ACK_ENDPOINT` |
| `sideshift-ack` | Worker 名 | `wrangler.toml` 的 `name`；改名要同步改 `tail`/`rollback` 里那一个 |
| `<KV namespace id>` / `<KV preview id>` | 两串 KV 命名空间 id | `kv namespace create` 的输出，只填进 `wrangler.toml` |
| `account_id = "…"` | 账号 id | `wrangler whoami` 的输出，只进 `wrangler.toml` |
| `$AckKey` / `$ACK_KEY` | 编辑口令 | **文档里没有**。只经 `wrangler secret put` 的交互输入或 `Read-Host` 进内存 |
| `dev-local-key` | 本地 dev 的假口令 | 只存在于 gitignore 掉的 `.dev.vars`，与线上口令无关 |

真值不往这份文档、提交信息、日志里写——域名也不算公开信息（它是客户端二进制里的常量，写进公开 README 等于把迁移成本挂到别人能看到的页面上）。跑命令时把占位域换成你的那一个即可，**换完别提交**，想退回来：`git checkout -- deploy/ack/README.md`。

---

## 一、一次性初始化（CLI 路线，约 10 分钟）

`wrangler` 不用装：`npx` 现取（本机实测 4.144.0）。前置只有两件事——一个 Cloudflare 账号，以及 `ack.example.com` 这个 zone 在**同一个账号**下（自定义域是靠 deploy 时那条 `routes` 创建的，zone 不在就得先加域）。

下面所有 wrangler 命令都吃**当前目录**的 `wrangler.toml`，所以先 `Set-Location deploy\ack`；不想切目录就每条末尾加 `-c deploy/ack/wrangler.toml`。这一句对本节所有命令成立。

### 1. 登录

```powershell
Set-Location deploy\ack
npx wrangler login          # 弹浏览器授权，本地起一个回调端口；授权完窗口自己收回
npx wrangler whoami
```

判据：`whoami` 列出的 `account_name` + `account_id` 是你以为的那一个。账号多于一个时，把 `account_id = "…"` 写进 `wrangler.toml` 顶层，后面所有命令都吃它。

### 2. 建 KV 命名空间，两个 id 填进配置

```powershell
npx wrangler kv namespace create ACK
```

输出里两串 id：`id` 是**线上**那一档、`preview_id` 是 `wrangler dev` 本地用的那一档，逐字填进本目录 `wrangler.toml` 的这两行（现在是占位符）：

```toml
[[kv_namespaces]]
binding = "ACK"
id = "<KV namespace id>"          # ← 线上
preview_id = "<KV preview id>"    # ← 本地 dev
```

- **绑定名必须是 `ACK`**：`worker.js` 读的是 `env.ACK`，改名等于把 Worker 变成"没有 KV"（表现是 `/contributors.json` 恒 404，而不是报错）。
- 想少抄一次可以加 `--update-config --binding ACK`，但跑完 `git diff deploy/ack/wrangler.toml` 看一眼：配置里已经有那一段，它再追加第二段就是配错（同一 binding 两段，取哪段没保证）。手填最稳。

### 3. 口令与变量

```powershell
npx wrangler secret put SECRET_TOKEN    # 交互式粘贴；口令不进命令行、不进 shell 历史
npx wrangler secret list                # 判据：只列名字，看不到值
```

| 变量 | 放哪 | 要不要 |
| --- | --- | --- |
| `SECRET_TOKEN` | `secret put`（Secret 类型） | **必需**，`/submit` 的读写都考它 |
| `MOJANG_TOKEN` | `secret put` | 选填。正版验证查档要发行商配对的账号；没有它就把全部 `minecraftId` 填 `false`，带了也不保证能过 |
| `ACK_CDN` | `wrangler.toml` 的 `[vars]`（明文） | 只有你要自带头像、**且没有自定义域**时才填。头像与端点同源 ⇒ 不填 |

### 4. 部署

```powershell
npx wrangler deploy --dry-run --outdir dist   # 上线前先看配置形状，不产生任何写入
npx wrangler deploy
```

dry-run 会把绑定的资源逐行打出来（`env.ACK (…) KV Namespace`、`env.ACK_CDN (…) Environment Variable`）。`dist/` 是它的产物，不用提交。

### 5. 自定义域（定案是"必须自定义域"，别再往 workers.dev 上推）

`wrangler.toml` 顶层加这一段，再 `npx wrangler deploy`：

```toml
routes = [
  { pattern = "ack.example.com", custom_domain = true }
]
```

两条判据：控制台 Workers → `sideshift-ack` → **Settings → Domains & Routes** 里出现 `ack.example.com`；以及

```powershell
curl.exe -sSI https://ack.example.com/contributors.json
```

回 `HTTP/1.1 404` 且带 `server: cloudflare`。为什么域这件事不能将就，见文末「域名与大陆可达性」。

### 6.「部署完成」的判据

```powershell
$AckUrl = "https://ack.example.com"

curl.exe -sS -i "$AckUrl/contributors.json"                # 404 + x-sideshift-ack: not-found —— 404 才是预期的完成态
curl.exe -sS -i "$AckUrl/submit"                           # 401 {"error":"unauthorized"}（没带口令）
curl.exe -sS -i -X OPTIONS "$AckUrl/skins"                 # 204 + access-control-allow-origin: *
curl.exe -sS -i -X OPTIONS "$AckUrl/submit"                # 404 —— 带口令的路不该响应预检
```

`/skins` 是唯一公开、又会真打 Mojang 的口，单独验一次（**JSON body 走临时文件**，PowerShell 5.1 会把参数里的双引号吃掉）：

```powershell
'{ "names": ["Notch"] }' | Out-File -Encoding ascii "$env:TEMP\skins.json"
curl.exe -sS -X POST "$AckUrl/skins" --data-binary "@$env:TEMP\skins.json"
```

期望 `{"people":{"Notch":{"uuid":"…","textures":"https://textures.minecraft.net/texture/…"}}}`。
三种失败各有含义：`unknown-player`＝名字不存在（**不落表**）；`mojang-rate-limit`＝Mojang 限流，稍后再试；`mojang-unreachable`＝这台 Worker 的出口到不了 api.mojang.com。

打过一次之后 KV 里应当多出一档皮肤缓存，这条同时验 KV 绑定真的能写：

```powershell
npx wrangler kv key get "skin:notch" --binding ACK --remote --text
```

---

## 二、日常：推送与修改名单

工作姿势：**改动只走一份本地文件**，别在命令行里拼 JSON——先备好文件、再校验、再推、再读回。

会话内先把变量摆好。**这一节在仓库根目录跑**（相对路径都按这里算，上一节那个 `Set-Location deploy\ack` 要先退回来）：

```powershell
Set-Location <你的仓库根>               # 就是含 `deploy\ack` 的那一层
$AckUrl = "https://ack.example.com"
$AckKey = Read-Host "编辑口令"          # Read-Host 的输入不写进 PSReadLine 历史，比手打省事
$list = "$PWD\deploy\ack\contributors.local.json"
```

**变量和用它的命令必须在同一个窗口**。换过会话 `$list` 就是空的，`--data-binary "@$list"` 会展开成光一个 `@`，报的是「reading a file」那一类错，跟口令和内容都无关。

### 1. 备好本地那一份

线上已经有人 ⇒ 先拉下来改：

```powershell
curl.exe -sS -o $list "$AckUrl/submit?key=$AckKey"
```

**存盘只认 `-o`**，别用管道（`| Set-Content` / `| Out-File`）——原因和实测字节见文末「PowerShell 5.1 的口径」第 3 条。

三个分支按你的意图选：

| 线上状态 / 你的意图 | 起步方式 |
| --- | --- |
| 已有人，要增删改 | 上面那条 GET 落 `$list`（回空串＝线上还没写过，走下一行） |
| 第一次，想直接有人 | `Copy-Item deploy\ack\contributors.sample.json $list`，把示例行换成真人 |
| 想让它空着（还没有贡献者，或清空） | 手写一份 `{"version":"…","people":[]}` 到 `$list`；这一档**必须推**，理由见本节第 7 条 |

无论哪个分支，下一步之前先验文件真的在：

```powershell
Test-Path $list
```

判据：`True`。出 `False` 就别往下走了，那正是 `--data-binary: error encountered when reading a file`（curl 退出码 26）的唯一成因。

### 2. 形状与字段

```jsonc
{
  "version": "1.0.0-r1",
  "people": [
    { "name": "player_one", "minecraftId": true },                     // 取皮肤做 3D 头
    { "name": "贡献者甲", "minecraftId": false },                     // 无头像 ⇒ 名字首字块
    { "name": "贡献者乙", "avatar": "https://{ENDPOINT_HOST}/avatars/b.png", "minecraftId": false }
  ]
}
```

`/submit` 写入时按这个闸（`worker.js` 的 `validateList`）：

| 字段 | 规则 |
| --- | --- |
| `version` | 非空字符串。**约定** `主版本-r修订号`；它只是字符串，客户端只做 `!=` 比较、界面不读它 |
| `people` | 数组（**空数组合法**）；顺序即屏上顺序 |
| `name` | 必填，非空字符串 |
| `minecraftId` | 必填，只能是 `true` / `false` 字面量——不能省、不能写 `0/1/"yes"` |
| `avatar` | 可整条不写；写了必须是字符串 |
| ~~`id`~~ | **不要加**。React key 用名字，只有重名时才挂一个「第几次出现」的次序，所以中间插一个人不会让后面整批重挂 |

关于 `version`：**改完名单要上线，就把它的 `-rN` 抬一格**。客户端只在版本串变了时重写本机快照（`core/ack.rs` 的 `should_store`），不抬那一格，用户端下次拿到也不落盘。

关于 `{ENDPOINT_HOST}`：由 Worker 换成 `ACK_CDN` 的主机；Worker 没换成功时，客户端还会按自己那侧的端点常量再换一次。**两种情况都不会把字面量漏进界面**——换不出来就是主机不合法，那一条头像按"没有头像"处理，落名字首字块。

### 3. 出网前自检（两条，把 400 挡在家里）

```powershell
node --input-type=module -e "import fs from 'node:fs'; const {validateList}=await import('./deploy/ack/worker.js'); console.log(validateList(fs.readFileSync('deploy/ack/contributors.local.json','utf8')) ?? '形状合格')"
```

判据：打印 `形状合格`。不合格它会把第一条没过的原因原样说出来（例如 `people[2].minecraftId 必须是 true 或 false`）。这条吃的是**线上那份校验代码**，不是文档里的转述。

再排掉 BOM（Notepad 和 5.1 的 `Set-Content -Encoding utf8` 都会写 UTF-8 BOM，`JSON.parse` 见 BOM 直接抛 ⇒ Worker 回 400）：

```powershell
$b = [IO.File]::ReadAllBytes($list); "{0:X2} {1:X2} {2:X2}" -f $b[0], $b[1], $b[2]   # EF BB BF 就是带 BOM
[IO.File]::WriteAllText($list, [IO.File]::ReadAllText($list), (New-Object System.Text.UTF8Encoding($false)))  # 去掉后原路径写回
```

### 4. 存一份上一版，再推

KV 没有版本历史，能回滚的东西只有你手上那一份旧文件——所以**留档在推之前，不在推之后**：

```powershell
Copy-Item $list "$PWD\deploy\ack\contributors.prev.json" -Force
curl.exe -sS -X POST --data-binary "@$list" "$AckUrl/submit?key=$AckKey"
```

- `@` 前必须加引号：PowerShell 会把开头的 `@` 当 splatting 运算符。
- 成功回执：`{"ok":true,"version":"…","people":N}`。**`N` 是你表里的人数，想清空时它就是 0**，别把「N≥1」当成功条件。

### 5. 读回核对（**这步不许省**）

```powershell
curl.exe -sS "$AckUrl/contributors.json"
```

肉眼确认中文昵称还是原样。这是"编码没被中途改写"的唯一外显证据；`/submit` 推成功不代表落进去的是你想要的那串字。想绕过边缘缓存直读 KV 本体，用 `curl.exe -sS "$AckUrl/submit?key=$AckKey"`（那条是 `no-store`）。

### 6. 生效时间：别把延迟判成失败

前两层是延迟（叠起来 1~2 分钟），第三层不是延迟、是"它压根不会自己去问第二遍"：

1. Worker 给 `contributors.json` 挂 `cache-control: public, max-age=60` ⇒ 边缘最多压 60 秒；
2. KV 写入到全球一致本身有秒级延迟；
3. 客户端**每次启动只后台对账一次**（`useContributors.ts` 的模块级 `reconciled`）：切走再回关于页不会重拉。所以刚推完想在屏上确认，要么重开客户端，要么点界面上那颗「重新获取」。

想看边缘那一层：

```powershell
curl.exe -sSI "$AckUrl/contributors.json" | Select-String "cf-cache-status|age|cache-control"
```

### 7.「名单空着」这一档：推与不推是两句不同的实话

界面上非「有人」的情况有两句文案，走哪句由**你推不推表**决定：

| 线上状态 | 客户端拿到 | 屏上显示 |
| --- | --- | --- |
| KV 没这个键（从没推过） | `GET` 回 **404**，而非 2xx 一律算错（`ack.rs` 的 `fetch_list`） | `当前无法显示鸣谢名单` + **一颗「重新获取」** |
| 推过 `{"version":"…","people":[]}` | 请求成功、名单为空 | `名单还没有登记的贡献者`，**不给**重试（按下去只会拿回同一份空表） |

⇒ **想让名单空着，就得把空表推上去**，否则"还没有贡献者"会被演成"出故障了"。

那条真代价只落在离线路：`should_store` 不许「本机有人、新表为空」覆盖本机快照，所以在线那一路每次拿空表照样上屏（对的），而**已经存过人员快照的机器在离线或拉取失败时会回读旧名单**；快照在配置目录、不在任何可清理类目，本机没有出口能清它。发版之后再清空名单才会咬人。

### 8. 报错对照表

| 现象 | 意思 | 动作 |
| --- | --- | --- |
| `curl: option --data-binary: error encountered when reading a file`（退出码 26） | `@` 后面那个**本地文件不存在**，或 `$list` 在这个窗口里是空的 | `Test-Path $list` 先验；变量与命令同窗口 |
| `{"error":"unauthorized"}` + 401 | 口令没带、或不对 | 重取 `$AckKey`（换过会话就没了） |
| `{"error":"people[2].minecraftId 必须是 true 或 false"}` + 400 等 | `validateList` 没过，**整趟拒掉、线上旧表一字不动** | 照 `error` 那句改，再推一次，没有副作用 |
| `{"error":"不是合法 JSON"}` + 400，但本地看着没问题 | 十有八九是 **BOM** | 用第 3 条那两行量头三字节 |
| `404 {"error":"list-not-found"}` + `x-sideshift-ack: not-found` | 端点活着，只是**线上还没写过名单** | 首次部署后这是预期态，见第 7 条 |
| 推成功了，界面还是旧的 | 边缘 60 秒 + 客户端每次启动只对账一次 | 重开客户端或点「重新获取」，不是失败 |
| 推成功了，控制台 KV 面板还是旧的 | 面板是你打开那一页时的快照，不跟着 `put` 刷新 | `Ctrl+F5`；判据用 `GET /submit`（`no-store`）那条 |
| `wrangler kv … --remote` 报找不到命名空间 | `wrangler.toml` 里 `id` / `preview_id` 还是占位符 | 填真 id，或者干脆走 `/submit`（不吃这个配置） |

### 9. 旁路：不经过 `/submit` 直接读写 KV

口令丢了、或想跳过校验硬改时使用。代价：**绕过 `validateList`，写坏了没人拦**；而且它吃 `wrangler.toml` 的两个 id，占位符没填就根本跑不通。

```powershell
Set-Location deploy\ack                     # 或者每条末尾加 -c deploy/ack/wrangler.toml
npx wrangler kv key put "contributors.json" --path $list --binding ACK --remote
npx wrangler kv key get "contributors.json" --binding ACK --remote --text
npx wrangler kv key list --binding ACK --remote
npx wrangler kv key list --binding ACK --remote --prefix "skin:"   # 只看皮肤缓存那一档
npx wrangler kv key delete "skin:notch" --binding ACK --remote     # 键名小写（`SKIN_PREFIX + name.toLowerCase()`）
```

**`--remote` 别省**：它打的是 `wrangler.toml` 里 `id` 那一档（线上），`--preview` 是 `preview_id`，`--local` 是 `.wrangler/state` 的本地模拟。皮肤缓存想提前作废（改名刚发生、旧记录指着错的人，而保鲜期 90 天）就 `kv key delete` 那一档，下一次 `/skins` 会自己回源。名单那一档固定叫 `contributors.json`，皮肤档是小写加 `skin:` 前缀（`worker.js` 的 `SKIN_PREFIX`）。

### 10. bash（git-bash）形态

```bash
curl -s "https://ack.example.com/submit?key=$ACK_KEY" -o deploy/ack/contributors.local.json
curl -s -X POST --data-binary "@deploy/ack/contributors.local.json" "https://ack.example.com/submit?key=$ACK_KEY"
```

git-bash 里 `@` 后面跟的是 **bash 路径**（`@deploy/ack/…`），贴 Windows 路径会「file not found」。

---

## 三、日常：修改 Worker 本身

```powershell
Set-Location deploy\ack
node test.mjs                                  # 逻辑层单测：不联网、不打 Mojang、不用 wrangler
npx wrangler dev                               # 本地跑，KV 用 .wrangler/state 的本地模拟
npx wrangler dev --remote                      # 跑到 Cloudflare 全局网络上、直读写线上 KV
npx wrangler dev --port 8790                   # 端口占用时自己指定
npx wrangler deploy
npx wrangler tail sideshift-ack --status error # 只看报错的调用；排障时开着这一条
npx wrangler deployments list                  # 历史版本与 version-id
npx wrangler rollback <version-id> -m "回滚原因"
```

本地起 dev 时口令走 `.dev.vars`（仓库已 gitignore），内容与变量同名。值随你定，**但别用线上那一个**（本地那份会进文件、也会出现在下面那条 URL 的 query 里）：

```
SECRET_TOKEN=dev-local-key
```

于是本地那一趟是（地址以 `wrangler dev` 启动时打印那行为准，下面按默认的 8787 写；`$list` 是上一节那个绝对路径）：

```powershell
curl.exe -sS -X POST --data-binary "@$list" "http://127.0.0.1:8787/submit?key=dev-local-key"
```

**dev 默认那一档的表不会上线**：写的是 `.wrangler/state` 里的本地模拟，别在本地验完就以为发布了。反过来 `--remote` 是真写线上——用它之前想清楚，它读写的就是 `contributors.json` 那一档。

`test.mjs` 覆盖的是那些"错了会静默污染数据或白烧额度"的分支：失败不落表、命中不外呼、批量去重与上限、口令比较、形状校验（含"校验失败不许覆盖已有名单"）、贴图域校验、透传不改写、CORS 范围、缺 KV 绑定不抛异常。它用假 `fetch` 计数，也不依赖 Node 版本。判据：最后一行打出 `N 组通过`；任何一条断言不过会直接抛出、看不到这一行。**改完 `worker.js` 先跑它，再 deploy。**

改字段或路由时，**这四处是一个动作**，别漏：

1. `worker.js` 的 `validateList`（写入口的形状）＋ `test.mjs` 里对应那组断言；
2. `src-tauri/src/core/ack.rs` 的解析与 `ACK_ENDPOINT`（客户端消费方）；
3. `contributors.sample.json`（形状基准那份，得跟校验一致）；
4. 本 README 的「路由与行为」表和「形状与字段」。

**推名单不需要做这一节的任何事。** `GET /contributors.json` 每次请求现读 KV，所以数据写进去旧代码立刻就在发新数据；只有改了 `worker.js`、`wrangler.toml` 的 `[vars]` / `[[kv_namespaces]]`（`ACK_CDN` 是部署时烘进 `env` 的）、或 secret 才要重新 deploy。

---

## 路由与行为

| 路由 | 作用 |
| --- | --- |
| `GET /contributors.json` | 名单，KV 原样文本；没发布过 ⇒ 404 `{"error":"list-not-found"}` + 响应头 `x-sideshift-ack: not-found` |
| `GET /submit?key=` | 读当前名单文本（空串＝还没写过）；`no-store`，给编辑页用 |
| `POST /submit?key=` | 写名单（校验后原样存；校验失败不动旧表） |
| `POST /skins` | `{"names":["Notch",…]}` ⇒ `{"people":{"Notch":{"uuid":"…","textures":"https://textures.minecraft.net/texture/…"}}}`，失败项 `{"error":"…"}` |

- `/skins` 缓存优先：命中未过期 ⇒ **零外呼**；未命中才查 Mojang 并回写 KV。批量上限 64（超了回 `too-many-names`），空 `names` 回 `names-required`，**串行**（并发会一次烧光共用 Worker 的限流额度），同名去重。
- 缓存保鲜期 90 天：玩家会改名，名字被旁人接手后旧记录会指向错的人；过期重查（提前作废见上面 `kv key delete`）。
- 失败不落表（404 / 429 / 5xx / 空 body / 非法 body 一律不写 KV）。
- 只有 `POST /skins` 开 CORS（`*`），`OPTIONS` 也只对它回 204；其余路径的 `OPTIONS` 一律 404。两个带口令的口**不开 CORS** 也**不响应预检**：跨源请求在浏览器那一侧就失败，口令不会变成任意网页能碰的东西（预检排在口令校验之前，否则 401 会替我们招认这个路径存在）。名单那条读口同样不开 CORS——它是 Rust 侧 reqwest 直取，不过同源策略；**头像能不能显示取决于 CORP，不取决于 ACAO**（见下节）。
- **不公开** `/avatar/*`、`/texture/*` 这类取字节的代理路由：那会把 Worker 变成任意 URL 代理，是 SSRF 放大器，还白白吃掉出口带宽。

## 域名与大陆可达性

定案的是**形态**不是名字：端点必须挂在一个自定义域上。本文档一律写占位符 `ack.example.com`；**真实域名只存在于两处**——`src-tauri/src/core/ack.rs` 的 `ACK_ENDPOINT` 常量，和 Cloudflare 那边已绑的 route。

`*.workers.dev` 为什么不行，两条独立的理由：

1. **大陆会被 DNS 投毒**：解析回来的不是 Cloudflare 的地址而是黑洞地址，于是 `curl` 报 `(7) Could not connect to server` / `(28) timed out`——**跟 Worker 有没有部署对毫无关系**。判据是**域名被针对，不是 IP 段被针对**：Cloudflare 的 IP 本身在这台机器上可达（`www.cloudflare.com` → 200、`api.modrinth.com` → 能连上），同机走本地代理（Clash 那类，`127.0.0.1:7890`）也立刻通：

   ```powershell
   curl.exe -x http://127.0.0.1:7890 -s "https://<你的worker域名>/contributors.json"
   ```

2. **边缘强制注入 `Cross-Origin-Resource-Policy: same-origin`**，这个头**优先于** `<img>` 的 `referrerpolicy="no-referrer"` ⇒ 自带头像打不开（curl 却 200 就是这个原因）。而且 `*.workers.dev` **配不了自定义域**（Cloudflare 的限制），一个子域绑自定义域是整套迁移。Pages 不注入它，所以「图放 Pages、端点留 workers.dev」曾经是退路。

⇒ 自定义域解析回 Cloudflare 地址段，同机**不吃任何代理**直连秒回 `{"error":"list-not-found"}`（这是一个真域上的实测结论，那一个真值不在本文档里）。**头像与端点同源 ⇒ `{ENDPOINT_HOST}` 直接可用、`ACK_CDN` 不用填、`ACK_AVATAR_HOSTS` 不用设**（客户端白名单从端点常量推出，见 `avatar_allowed`）。

- **应用侧仍要知道**：客户端是 reqwest，走系统 DNS，**不读浏览器的代理设置**（那是 WinINET，不是环境变量）。所以将来若端点退回 workers.dev 那类被投毒的域，开着 Clash 自测也一样连不上——那种情况下要开 TUN/虚拟网卡，或在跑 `pnpm tauri dev` 的终端里先设 `$env:HTTPS_PROXY="http://127.0.0.1:7890"`。
- **还缺的一块**：Worker **不服务 `/avatars/*`**（故意的——那等于把它变成任意 URL 的字节代理）。所以真要用自带头像，得再选一条存放路：R2 桶 + 本域名的路由（只按 key 取、URL 不由 payload 给），或另开一个 Pages 站（那就需要设 `ACK_AVATAR_HOSTS`）。在这一条定下来之前，上面「形状与字段」里那种 `avatar` 推上去只会让那个人落首字块，不会 500、也不污染别人——即"能写、不显示"。

## 换机与失效

- **端点或域名换了** ⇒ 改 `wrangler.toml` 的 `routes`、改 `src-tauri/src/core/ack.rs` 的 `ACK_ENDPOINT` 常量并重新打包。老版本客户端会一直 404、一直显示「鸣谢名单不可见」——**这是已知且已接受的代价**，代码里不做多端点回落。
- **KV 绑定被删** ⇒ `/contributors.json` 恒 404，`/submit` 读写 500。客户端**没有内置种子数据**（名单不落进安装包是这轮的定案），所以它靠的是本机那一份快照：装过并成功拉取过一次的用户无感；从没拉成功过的用户看到「当前无法显示鸣谢名单」+ 重新获取。⇒ 这条链没有"出厂兜底"，**部署完成后务必自己开一次客户端确认快照已落盘**，再删绑定测试。
- **`SECRET_TOKEN` 泄漏** ⇒ 对方可任意改写名单（含 `{ENDPOINT_HOST}`）。后果被客户端那道主机白名单兜住：注入的第三方主机头像一律无效，最坏是名单显示错人，不会变成图片追踪或跳转注入。处置就一条命令，无需改代码：

  ```powershell
  Set-Location deploy\ack
  npx wrangler secret put SECRET_TOKEN     # 同名再 put 一次＝覆盖
  ```

  轮换后旧口令立刻失效（下一次 `/submit` 回 401），手上那几个会话变量也要重取。

## PowerShell 5.1 的口径

抄命令前扫一遍这五条，栽的都是它们。

1. **`curl` 是 `Invoke-WebRequest` 的别名**：`-s` / `-X` 会被它当自己的参数吞掉并报 `PositionalParameterNotFound`。要写 `curl.exe`，参数才有 curl 的语义。
2. **续行用反引号 `` ` ``**（不是 `\`），而且反引号后面不能留空格。跨行的 `curl.exe` 长命令最容易栽在这一笔上——报的是「缺少参数」而不是"你续行写错了"。
3. **两种存盘方式都会搞坏中文**，所以本文档所有存盘只认 `curl.exe -o <文件>`（它直接把字节写盘、不经过任何解码）：
   - **管道**：`curl.exe … | Set-Content` / `| Out-File` 会先把字节按**控制台代码页**解码（这台机实测是 **gb2312**，查：`[Text.Encoding]::Default.WebName`）。实测本该是 `E4 BD A0 E5 A5 BD`，走一趟管道落盘变成 `E6 B5 A3 E7 8A B2 E3 82 BD`——三重乱码，还附送 BOM 和 CRLF。而 `Out-File` 不加 `-Encoding` 时 5.1 的默认是 UTF-16LE，那种文件连 `JSON.parse` 都进不去。
   - **BOM**：`Set-Content -Encoding utf8` 在 5.1 里**写 BOM**（实测头三字节 `EF BB BF`），Worker 那边 `JSON.parse` 遇到 BOM 直接抛。写不带 BOM 的 UTF-8 用：`[IO.File]::WriteAllText($f,$s,(New-Object System.Text.UTF8Encoding($false)))`（实测头三字节 `7B 22 61`，即 `{"a`）。
   - 不想用 curl.exe 的场合，原生 cmdlet 这一条也行：

     ```powershell
     $u = "https://ack.example.com/submit?key=$AckKey"
     $b = [IO.File]::ReadAllText($list)   # ReadAllText 会吃掉 BOM
     Invoke-RestMethod -Method Post -Uri $u -Body $b -ContentType "application/json; charset=utf-8"
     ```

     `-ContentType` 那个 charset 不能省，否则中文进去的是问号。
4. **JSON body 别 inline**：5.1 传给原生程序时会吃掉参数里的双引号，`--data-binary '{"names":[]}'` 进去的是碎串。写成临时文件再 `--data-binary "@文件"`（见「部署完成判据」里的 `/skins` 那一验）。
5. **口令留在历史文件里**：`Read-Host` 取口令不进历史；手打过的话用完清一下——`Remove-Item (Get-PSReadLineOption).HistorySavePath`。

## 附：控制台手工路线（与 CLI 二选一，同一份配置）

1. Workers & Pages → Create → **Worker** → 新建，名字随意（CLI 那条叫 `sideshift-ack`）。
2. **Settings → Variables and Secrets**：`SECRET_TOKEN`（Secret，Generate 一个）、选填 `MOJANG_TOKEN`；只有自带头像且没有自定义域时才加明文变量 `ACK_CDN`。
3. **Settings → Integration → Workers KV Namespace**：Create namespace，名字随意，绑到本 Worker，**绑定名（Binding name）必须是 `ACK`**。
4. 把 `worker.js` 贴到 Code 里，Save and Deploy。
5. 自定义域在 **Settings → Domains & Routes** 里 Add → Custom Domain，填你的真实域名（本文档一律写作 `ack.example.com`）。
6. 打开 `https://ack.example.com/contributors.json`，浏览器显示 404 JSON `{"error":"list-not-found"}` —— **404 就是预期的"部署完成"**，别误判。

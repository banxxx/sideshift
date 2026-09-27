# 关于页「鸣谢名单」的 Cloudflare Worker

一份名单 JSON + Minecraft 皮肤反查缓存。**头像字节不过这里**：`/skins` 只回
`textures.minecraft.net` 的 URL，贴图由客户端直连 Mojang CDN。

## 部署（约 3 分钟）

1. Workers & Pages → Create → **Worker** → 新建，名字随意。
2. **Settings → Variables and Secrets**：
   - `SECRET_TOKEN` — 编辑口令，Generate 一个（Secret 类型）。
   - `ACK_CDN` — 只有你要自带头像时才需要：本 Worker 的对外主机名，用来换掉 JSON 里的
     `{ENDPOINT_HOST}`。**自定义域名优先**（原因见下面「自定义域」一节）。
   - `MOJANG_TOKEN` — 选填。正版验证查档要发行商配对的账号；没有它就把全部
     `minecraftId` 填 `false`。带了不保证能过。
3. **Settings → Integration → Workers KV Namespace**：Create namespace，名字随意，
   绑到本 Worker，**绑定名（Binding name）必须是 `ACK`**。
4. 把 `worker.js` 贴到 Code 里，Save and Deploy。
   （`wrangler.toml` 是走 CLI 部署时的同一份配置，二选一。）
5. 打开 `https://poso.us.ci/contributors.json`，浏览器会显示 404 JSON
   `{"error":"list-not-found"}` —— **404 就是预期的"部署完成"**，别误判。

## 发布名单

`GET /submit?key=<口令>` 读当前文本，编辑后 `POST` 回去。curl 版（改一处 URL 与口令，
可直接复制跑）：

```bash
# 读
curl -s "https://poso.us.ci/submit?key=<口令>"

# 写（文件见本目录 contributors.sample.json）
curl -sX POST "https://poso.us.ci/submit?key=<口令>" \
     --data-binary @contributors.json
```

写入时校验：JSON 合法性、`version` 非空字符串、`people` 是数组、每条 `name` 非空字符串、
`avatar` 给了就必须是字符串、`minecraftId` 必须是布尔。**名单文本原样存、原样透传**，
不重新序列化——改了 `minecraftId` 记得把 `version` 也改一下。

回 `{"ok":true,"version":"…","people":123}`；不合法时 400 带原因。

**第一次发版前先把这一节看完**：客户端在非「有人」的情况下有两句不同的实话，走哪句由你推不推表决定——

- **没推过（KV 未写）⇒ 404** ⇒ 客户端「一次数据都没拿到过」档：`当前无法显示鸣谢名单` + 重新获取。
- **推了 `{"version":"…","people":[]}`** ⇒ 请求成功、名单空 ⇒ `名单还没有登记的贡献者`，**不给**重试钮
  （按下去只会拿回同一份空表）。

空表不会覆盖本机已有的人员快照（`core/ack.rs` 的 `should_store`），但**屏上那一批仍会跟着远端变空**——
远端什么时候填上人，什么时候才恢复。

### 在 PowerShell 里跑（Windows 默认那个蓝底窗口）

上面两条是 bash 形态。**PowerShell 里 `curl` 是 `Invoke-WebRequest` 的别名**，`-s`/`-X` 会被它
当自己的参数吞掉并报 `PositionalParameterNotFound`。要写 `curl.exe`，而且 `@文件` 必须加引号
（PowerShell 会把开头的 `@` 当 splatting）：

```powershell
curl.exe -sS -X POST "https://<你的worker域名>/submit?key=<口令>" `
         --data-binary "@deploy\ack\contributors.sample.json"

curl.exe -s "https://<你的worker域名>/contributors.json"
```

不想用 curl.exe 就走原生 cmdlet——但**名单里有中文昵称时 `-Body` 传字符串在 Windows PowerShell
5.1 上编码出过岔子**，必须把 charset 显式写进 `-ContentType`，否则进去的是问号或乱码：

```powershell
$u = "https://<你的worker域名>/submit?key=<口令>"
$b = [IO.File]::ReadAllText("$PWD\deploy\ack\contributors.sample.json")
Invoke-RestMethod -Method Post -Uri $u -Body $b -ContentType "application/json; charset=utf-8"
```

写完**务必跑那条读口，肉眼确认中文昵称还是原样**——那是编码没被中途改写的唯一外显证据。
口令会留在 PSReadLine 的历史文件里，用完清一下：`Remove-Item (Get-PSReadLineOption).HistorySavePath`。

## JSON 的写法

```jsonc
{
  "version": "1.0.0-beta.1-r2",
  "people": [
    { "name": "Banxxx", "minecraftId": true },                       // 取皮肤做 3D 头
    { "name": "贡献者甲", "minecraftId": false },                     // 无头像 ⇒ 名字首字块
    { "name": "贡献者乙", "avatar": "https://{ENDPOINT_HOST}/avatars/b.png", "minecraftId": false }
  ]
}
```

- `{ENDPOINT_HOST}` 会由 Worker 换成 `ACK_CDN` 的主机；Worker 没换成功时，客户端还会
  按自己那侧的端点常量再换一次。**两种情况都不会把字面量漏进界面**：换不出来就是主机
  不合法，那一条头像按"没有头像"处理，落名字首字块。
- 顺序即屏上顺序，不要加 `id` 字段（React key 用名字，只有重名时才再挂一个「第几次出现」的次序，
  所以中间插一个人不会让后面整批卡重挂）。

## 域名（已定案：`poso.us.ci`，别再往 workers.dev 上退）

线上端点是 **`https://poso.us.ci/contributors.json`**，已写进 `src-tauri/src/core/ack.rs` 的
`ACK_ENDPOINT`。这一层定下来以后，前面为 workers.dev 准备的两处机制**同时作废**：

1. `*.workers.dev` **配不了自定义域**（Cloudflare 的限制），而一个子域绑自定义域是**整套迁移**；
2. workers.dev 还被边缘**强制**注入 `Cross-Origin-Resource-Policy: same-origin`，这个头**优先于**
   `<img>` 的 `referrerpolicy="no-referrer"` ⇒ 自带头像打不开（curl 却 200 就是这个原因）。
   Pages 不注入它，所以「图放 Pages、端点留 workers.dev」曾经是退路。

**头像与端点同源 ⇒ `{ENDPOINT_HOST}` 直接可用、`ACK_CDN` 不用填、`ACK_AVATAR_HOSTS` 不用设**
（客户端白名单从端点常量推出，见 `avatar_allowed`）。

> 还缺的一块：Worker **不服务 `/avatars/*`**（故意的——那等于把它变成任意 URL 的字节代理）。
> 所以真要用自带头像，得再选一条存放路：R2 桶 + 本域名的路由（只按 key 取、URL 不由 payload 给），
> 或另开一个 Pages 站（那就需要设 `ACK_AVATAR_HOSTS`）。这一条等要做时再定。

## 路由与行为

| 路由 | 作用 |
| --- | --- |
| `GET /contributors.json` | 名单，KV 原样文本；没发布过 ⇒ 404 `{"error":"list-not-found"}` + 响应头 `x-sideshift-ack: not-found` |
| `GET /submit?key=` | 读当前名单文本 |
| `POST /submit?key=` | 写名单（校验后原样存） |
| `POST /skins` | `{"names":["Notch",…]}` ⇒ `{"people":{"Notch":{"uuid":"…","textures":"https://textures.minecraft.net/texture/…"}}}`，失败项 `{"error":"…"}` |

- `/skins` 缓存优先：命中未过期 ⇒ **零外呼**；未命中才查 Mojang 并回写 KV。
  批量上限 64，**串行**（并发会一次烧光共用 Worker 的限流额度），同名去重。
- 缓存保鲜期 90 天：玩家会改名，名字被旁人接手后旧记录会指向错的人；过期重查。
- 失败不落表（404 / 429 / 5xx / 空 body / 非法 body 一律不写 KV）——
  否则一次网络抖动会被冻成"永久错误的 UUID"。
- 只有 `POST /skins` 开 CORS（`*`），`OPTIONS` 也只对它回 204；其余路径的 `OPTIONS` 一律 404。
  两个带口令的口**不开 CORS** 也**不响应预检**：跨源请求在浏览器那一侧就失败，
  口令不会变成任意网页能碰的东西（预检排在口令校验之前，否则 401 会替我们招认这个路径存在）。
  名单那条读口同样不开 CORS——它是 Rust 侧 reqwest 直取，不过同源策略；
  头像能不能显示取决于 CORP，不取决于 ACAO（见上「自定义域」）。
- **不公开** `/avatar/*`、`/texture/*` 这类取字节的代理路由：那会把 Worker 变成任意 URL
  代理，是 SSRF 放大器，而且白白吃掉出口带宽。

## 换机与失效

- **端点或域名换了** ⇒ 改 `src-tauri/src/core/ack.rs` 的 `ACK_ENDPOINT` 常量并重新打包。
  老版本客户端会一直 404、一直显示「鸣谢名单不可见」——**这是已知且已接受的代价**，
  代码里不做多端点回落。
- **KV 绑定被删** ⇒ `/contributors.json` 恒 404，`/submit` 读写 500。
  客户端**没有内置种子数据**（名单不落进安装包是这轮的定案），所以它靠的是本机那一份快照：
  装过并成功拉取过一次的用户无感；从没拉成功过的用户看到「当前无法显示鸣谢名单」+ 重新获取。
  ⇒ 这条链没有"出厂兜底"，**部署完成后务必自己开一次客户端确认快照已落盘**，再删绑定测试。
- **`SECRET_TOKEN` 泄漏** ⇒ 对方可任意改写名单（含 `{ENDPOINT_HOST}`）。
  后果被客户端那道主机白名单兜住：注入的第三方主机头像一律无效，最坏是名单显示错_names_，
  不会变成图片追踪或跳转注入。轮换该变量即可，无需改代码。

## 中国大陆可达性：workers.dev 不通，自定义域已解决（都实测过）

`*.workers.dev` 在大陆会被 **DNS 投毒**：解析回来的不是 Cloudflare 的地址，而是黑洞地址，
于是 `curl` 报 `(7) Could not connect to server` / `(28) timed out`，**跟 Worker 有没有部署对毫无关系**。
同机走本地代理（Clash 那类，`127.0.0.1:7890`）就立刻通：

```powershell
curl.exe -x http://127.0.0.1:7890 -s "https://<你的worker域名>/contributors.json"
```

⇒ 判据是**域名被针对，不是 IP 段被针对**：Cloudflare 的 IP 本身在这台机器上可达
（`www.cloudflare.com` → 200、`api.modrinth.com` → 能连上）。所以正解是**给 Worker 配自定义域**，
让它解析到 Cloudflare 的 IP——这一条同时把头像的 CORP 问题一并解决（见上「域名」）。

**已收口**：`poso.us.ci` 解析回 Cloudflare 地址段，同机**不吃任何代理**直连 1.06 秒回
`{"error":"list-not-found"}` ⇒ 壳里的 reqwest 同样直连即可，**不需要 TUN、也不用设 `HTTPS_PROXY`**。

- **应用侧仍要知道**：客户端是 reqwest，走系统 DNS，**不读浏览器的代理设置**（那是 WinINET，不是环境变量）。
  所以将来若端点退回 workers.dev 那类被投毒的域，开着 Clash 自测也一样连不上——
  那种情况下要开 TUN/虚拟网卡，或在跑 `pnpm tauri dev` 的终端里先设
  `$env:HTTPS_PROXY="http://127.0.0.1:7890"`。

## 本地跑与测

```bash
npx wrangler dev            # 本地起 Worker（KV 用 .wrangler 持久化模拟）
node test.mjs               # 逻辑层单测：resolve 缓存/去重/失败不落表、validateList、口令比较
```

`test.mjs` 不依赖网络（假 `fetch` 计数）也不依赖 Node 版本（用 `Object.assign` 造 env，
不用 `env.ACK ??=` 那类新语法）。

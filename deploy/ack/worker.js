/**
 * 关于页「鸣谢名单」的 Cloudflare Worker：一份名单 JSON + Minecraft 皮肤反查缓存。
 *
 * 三条前提（都是上一轮谈定的，代码只按这三条写）：
 *  1. **只有这一个写入口**。`/submit` 带 `SECRET_TOKEN` 才算写；`/skins` 内部维护 KV，
 *     键名与值形状全在本文件里 ⇒ 「数据格式永远由服务端说了算」不是一句口号，是因为
 *     不存在别人能写进来的路。名单**原样透传、一个字段都不碰**，改错了责任清楚。
 *  2. **贴图字节不过这里**。`/skins` 只回 `textures.minecraft.net` 那个 URL，字节由客户端
 *     直连 Mojang CDN ⇒ 我们这边零出口带宽。搬运贴图只会多花带宽，收益一个都没有。
 *  3. **Mojang 的错误绝不写进表**。非 200 时 `resolve` 直接返回，一次 `kv.put` 都不发生。
 *     否则"名字暂时查不到"会变成"永久返回一个错误的 UUID"——那比查不到难修一个数量级。
 *
 * 路由：
 *   GET  /contributors.json            名单（KV 原样文本；404＝还没写过）
 *   GET  /submit?key=…                 读名单（给编辑页用）
 *   POST /submit   (body=名单文本)      写名单，先按 validateList 校验形状
 *   POST /skins    (body={"names":[…]}) 皮肤反查；只回 {name:{uuid|textures|error}}
 *
 * 变量（Workers → Settings → Variables）：
 *   ACK_CDN       可选，本 Worker 的对外主机名（如 ack.example.com，或 workers.dev 全名）。
 *                 只在**没有自定义域**时要填：端点主机与头像主机不同源时，客户端的头像
 *                 白名单推不出头像那个域（见 src-tauri/src/core/ack.rs 的 `avatar_allowed`），
 *                 这个变量就是把第二个域告诉它的出口。填了不产生别的副作用——/skins 不读它。
 *   SECRET_TOKEN  编辑口令（Secret 类型）。
 *   MOJANG_TOKEN  选填。正版验证查档要发行商配对的账号，没有它就把 `minecraftId` 全填 false；
 *                 带了就带上，不保证能过。
 *
 * KV 命名空间绑定：`ACK`（必须建，与 Worker 同项目）。
 */

const LIST_KEY = "contributors.json";
const SKIN_PREFIX = "skin:";
/** 一条皮肤档的保鲜期：玩家会改名，名字被旁人接手后旧记录就指向错的人。
 *  90 天重查一次——一次 KV 读加极偶发一次外呼，换来"最坏错三个月"而不是"错到永远"。 */
const SKIN_TTL_S = 90 * 24 * 3600;
/** 单次批量上限：没有它，任何人写个脚本就能拿这台 Worker 当免费代理去刷 Mojang */
const BATCH_MAX = 64;
/** 字符集与 16 上限挡掉的是明显垃圾（空格、中文、@、超长串），一次网都不出。
 *  下限**故意不设**：是否还存在一两个字符的老账号我没有查实，而猜错方向的代价不对称——
 *  拦错了 = 某个贡献者的皮肤永远出不来（界面退回首字母，用户看不出原因）；
 *  放过了 = Mojang 回一个 404，而 404 不落表。所以这里宁可放过。 */
const MC_NAME = /^[A-Za-z0-9_]{1,16}$/;

const json = (body, status = 200, headers = {}) =>
    new Response(JSON.stringify(body), {
        status,
        headers: { "content-type": "application/json; charset=utf-8", ...headers },
    });

const text = (body, contentType, maxAge, status = 200, headers = {}) =>
    new Response(body, {
        status,
        headers: {
            "content-type": contentType,
            "cache-control": `public, max-age=${maxAge}`,
            ...headers,
        },
    });

/** 常量时间比较：口令这种东西不该走 `===` 的短路 */
function tokenOk(given, expect) {
    if (!expect || typeof given !== "string" || given.length !== expect.length) return false;
    let diff = 0;
    for (let i = 0; i < given.length; i++) diff |= given.charCodeAt(i) ^ expect.charCodeAt(i);
    return diff === 0;
}

/**
 * 只取主机名；解析不出来（空串、根本不是 URL）就返回空串 ⇒ 调用方宁可不替换。
 * 允许不带协议（`ack.example.com`）：这是面板里最自然的填法，而占位符换不出来等于
 * 所有自带头像被客户端判成非法，所以这里宽一次，而不是等人踩了再改文档。
 */
function safeHost(raw) {
    const v = String(raw ?? "").trim();
    if (!v) return "";
    try {
        return new URL(v).hostname;
    } catch {
        try {
            return new URL(`https://${v}`).hostname;
        } catch {
            return "";
        }
    }
}

/** Mojang 这两个查询接口都不需要 Authorization ⇒ 没配过变量就别带一个空头出去 */
function authHeader(env) {
    return env.MOJANG_TOKEN ? { Authorization: `Bearer ${env.MOJANG_TOKEN}` } : {};
}

/**
 * 名字 → UUID（命中缓存则一次外呼都不发）。`/skins` 与单测共用这一个实现，
 * 因为这条链最容易写歪的两处（404 不许落表、失败不许落表）就都在里面。
 */
export async function resolve(name, env, retain) {
    if (!MC_NAME.test(name)) return { error: "bad-name" };

    const key = SKIN_PREFIX + name.toLowerCase();
    let row = null;
    try {
        row = await env.ACK.get(key, "json");
    } catch {
        row = null; // 读不到就当未命中：宁可多查一次 Mojang，也不要面板抖一下就没名单
    }
    if (row && typeof row.u === "string" && Date.now() - (row.t ?? 0) < SKIN_TTL_S * 1000) {
        return { uuid: row.u, textures: row.x };
    }

    let res;
    try {
        res = await fetch(
            `https://api.mojang.com/users/profiles/minecraft/${encodeURIComponent(name)}`,
            { headers: authHeader(env) }
        );
    } catch {
        return { error: "mojang-unreachable" };
    }
    if (res.status === 429) return { error: "mojang-rate-limit" };
    if (res.status === 404) return { error: "unknown-player" }; // 查不到人：不落表，免得把拼错的名字冻三个月
    if (!res.ok) return { error: "mojang-" + res.status }; // 5xx 同样不落表：失败不该有缓存
    // 204 与空 body 是 Mojang 限流的另一副面孔 ⇒ 算失败，不算"成功但没有玩家"
    const raw = await res.text();
    if (!raw.trim()) return { error: "mojang-empty" };
    let profile;
    try {
        profile = JSON.parse(raw);
    } catch {
        return { error: "mojang-bad-body" };
    }
    if (!profile || typeof profile.id !== "string") return { error: "mojang-bad-body" };

    const fresh = { u: profile.id, n: name, t: Date.now() };
    // 存贴图 URL 等于把一个远端可控的外链写进我们自己的表、下次由我们发出去 ⇒ 回源校验，
    // 只认 Mojang 那个静态 CDN；不认就只存 uuid，让皮肤那条链自己失效
    const textures = await textureUrl(profile.id, env);
    if (textures) fresh.x = textures;
    try {
        const put = env.ACK.put(key, JSON.stringify(fresh));
        // 响应可以先走，写表挂到 waitUntil；拿不到 waitUntil（单测）时就地等完，别留悬空 Promise
        if (retain) retain(put);
        else await put;
    } catch {
        /* 写不进去只影响下次要不要重查，不影响本次结果 */
    }
    return { uuid: profile.id, textures: fresh.x };
}

/** UUID → 贴图 URL，两道校验：必须 https，主机必须正好是 textures.minecraft.net */
async function textureUrl(uuid, env) {
    try {
        const res = await fetch(
            `https://sessionserver.mojang.com/session/minecraft/profile/${encodeURIComponent(uuid)}`,
            { headers: authHeader(env) }
        );
        if (!res.ok) return null;
        const body = await res.json();
        const props = Array.isArray(body?.properties) ? body.properties : [];
        const entry = props.find((p) => p?.name === "textures");
        if (!entry?.value) return null;
        const decoded = JSON.parse(atob(entry.value));
        // 皮肤站给的是 model 那套 UV；它没给时才回落 classic
        const url = decoded?.skins?.model?.url ?? decoded?.skins?.classic?.url;
        if (typeof url !== "string") return null;
        const u = new URL(url);
        return u.protocol === "https:" && u.hostname === "textures.minecraft.net" ? url : null;
    } catch {
        return null;
    }
}

/** 名单的写入口：只校验到「能不能喂给客户端」这一层，内容对不对归你 */
export function validateList(raw) {
    let doc;
    try {
        doc = JSON.parse(raw);
    } catch {
        return "不是合法 JSON";
    }
    if (!doc || typeof doc !== "object") return "顶层必须是对象";
    if (typeof doc.version !== "string" || !doc.version) return "version 必须是非空字符串";
    if (!Array.isArray(doc.people)) return "people 必须是数组";
    for (const [i, p] of doc.people.entries()) {
        if (!p || typeof p !== "object") return `people[${i}] 必须是对象`;
        if (typeof p.name !== "string" || !p.name.trim()) return `people[${i}].name 必须是非空字符串`;
        if ("avatar" in p && typeof p.avatar !== "string") return `people[${i}].avatar 给了就必须是字符串`;
        if (typeof p.minecraftId !== "boolean") return `people[${i}].minecraftId 必须是 true 或 false`;
    }
    return null;
}

export default {
    async fetch(request, env, ctx) {
        const url = new URL(request.url);

        // 预检排在所有路由之前：OPTIONS 是元数据请求，不该走到带口令那条路上去
        // （否则它会回 401，等于告诉别人这个路径存在、并且在考口令）。
        // 只有 /skins 需要预检：其余路径都是简单请求，浏览器根本不会发 OPTIONS。
        if (request.method === "OPTIONS") {
            if (url.pathname !== "/skins") return json({ error: "not-found" }, 404);
            // 通配 CORS 不能给那两条带口令的 GET——那等于任何网页都能来试口令
            return new Response(null, {
                status: 204,
                headers: {
                    "access-control-allow-origin": "*",
                    "access-control-allow-methods": "POST, OPTIONS",
                    "access-control-allow-headers": "content-type",
                    "access-control-max-age": "86400",
                },
            });
        }

        if (url.pathname === "/contributors.json") {
            if (request.method !== "GET" && request.method !== "HEAD") {
                return json({ error: "method-not-allowed" }, 405);
            }
            // 没建 KV 绑定：只回未命中，别在每次请求上抛异常（日志会被刷爆）
            const body = env.ACK ? await env.ACK.get(LIST_KEY, "text") : null;
            if (!body) {
                // 自定义头：客户端靠它认出"这是我们的端点说没有名单"，而不是别的域的 Not Found
                return json({ error: "list-not-found" }, 404, { "x-sideshift-ack": "not-found" });
            }
            // 名单文本原样透传（不 parse 再 stringify：那等于拿我们的 schema 改写你的数据）。
            // 只把 {ENDPOINT_HOST} 换成真实主机，好让头像与端点同源；挑不出主机就整串原样发
            const hint = env.ACK_CDN ? safeHost(env.ACK_CDN) : "";
            const out = hint ? body.replace(/\{ENDPOINT_HOST\}/g, hint) : body;
            // 不开 CORS：这份 JSON 由 Rust 侧的 reqwest 直取，不经过浏览器同源策略；
            // 而头像那条链要的是**过 CORP**（见 README 的自定义域一节），ACAO 治不了它。
            // 全站唯一需要 CORS 的是 /skins——那里是 3D 头那侧要跨域读 JSON。
            return text(out, "application/json; charset=utf-8", 60);
        }

        if (url.pathname === "/submit") {
            if (!tokenOk(url.searchParams.get("key"), env.SECRET_TOKEN)) {
                return json({ error: "unauthorized" }, 401);
            }
            if (request.method === "GET") {
                const body = await env.ACK.get(LIST_KEY, "text");
                // no-store 而不是 max-age=0：后者仍允许中间层存一份，编辑页会读到别人缓存里的旧名单
                return text(body ?? "", "application/json; charset=utf-8", 0, 200, {
                    "cache-control": "no-store",
                });
            }
            if (request.method !== "POST") return json({ error: "method-not-allowed" }, 405);
            const raw = await request.text();
            const bad = validateList(raw);
            if (bad) return json({ error: bad }, 400);
            await env.ACK.put(LIST_KEY, raw);
            const doc = JSON.parse(raw);
            return json({ ok: true, version: doc.version, people: doc.people.length });
        }

        if (url.pathname === "/skins" && request.method === "POST") {
            let body = null;
            try {
                body = await request.json();
            } catch {
                body = null;
            }
            const names = Array.isArray(body?.names) ? body.names : null;
            if (!names?.length) return json({ error: "names-required" }, 400);
            if (names.length > BATCH_MAX) return json({ error: "too-many-names", max: BATCH_MAX }, 400);
            // 去重只为挡一件事：客户端漏做一次 Set ⇒ 同名并发打 Mojang，白吃限流额度。
            // 它不解决"一批里两个名字撞同一个 UUID"——那是改名期的正常现象，无害
            const uniq = [...new Set(names.filter((n) => typeof n === "string"))];
            const out = {};
            // 串行：并发会把 Mojang 的限流额度一次烧光，而这台 Worker 是所有用户共用的
            for (const name of uniq) {
                out[name] = await resolve(name, env, (p) => ctx.waitUntil(p));
            }
            return json(
                { people: out },
                200,
                { "cache-control": "public, max-age=300", "access-control-allow-origin": "*" }
            );
        }

        return json({ error: "not-found" }, 404);
    },
};

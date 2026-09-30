/**
 * `node deploy/ack/test.mjs` —— Worker 逻辑层的单测，不依赖网络、也不依赖 wrangler。
 * 只测那些"错了会静默污染数据或白烧额度"的分支：失败不落表、命中不外呼、批量去重与上限、
 * 口令比较、形状校验、贴图域校验、透传不改写、CORS 范围、缺 KV 绑定不抛异常。
 * 真打 Mojang 的那一跳留给部署后手动验一次（见 README）。
 */
import assert from "node:assert/strict";
import worker, { resolve, validateList } from "./worker.js";

const LIST_KEY = "contributors.json";
const MOJANG = "https://api.mojang.com/users/profiles/minecraft/";
const SESSION = "https://sessionserver.mojang.com/session/minecraft/profile/";
const TEXTURE_URL = "https://textures.minecraft.net/texture/abc123";
const ID = "deadbeefdeadbeefdeadbeefdeadbeef";

/** 极简 KV 替身：只实现用得到的 get(text|json) / put，并暴露 store 供断言 */
function fakeKV(seed = {}) {
    const store = new Map(Object.entries(seed));
    return {
        store,
        async get(key, type) {
            const v = store.get(key);
            if (v == null) return null;
            return type === "json" ? JSON.parse(v) : v;
        },
        async put(key, value) {
            store.set(key, String(value));
        },
    };
}

const ok = (body) =>
    new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
const status = (code) => new Response("", { status: code });
const empty = (code = 204) => new Response("", { status: code });
const textureValue = (url = TEXTURE_URL) => btoa(JSON.stringify({ skins: { model: { url } } }));

/** 一条链都走得通的路由表；要测失败分支时按需覆盖其中一条 */
const healthy = [
    [MOJANG, () => ok({ id: ID, name: "Player0" })],
    [SESSION, () => ok({ id: ID, properties: [{ name: "textures", value: textureValue() }] })],
];

/** 换掉 globalThis.fetch 跑一段，返回它记录到的出网主机列表 */
async function withFetch(routes, fn) {
    const calls = [];
    const original = globalThis.fetch;
    globalThis.fetch = async (input) => {
        const url = String(input);
        calls.push(new URL(url).host);
        for (const [prefix, make] of routes) if (url.startsWith(prefix)) return make(url);
        throw new Error(`unexpected fetch: ${url}`);
    };
    try {
        return await fn(calls);
    } finally {
        globalThis.fetch = original;
    }
}

/** ctx.waitUntil 的替身：收集起来等完，否则 KV 写入会是悬空 Promise */
function ctxTracker() {
    const pending = [];
    return { ctx: { waitUntil: (p) => pending.push(Promise.resolve(p)) }, pending };
}

/** 打一次 worker.fetch；`{ kv }` 传既有 KV，`{ noKv: true }` 模拟没建绑定 */
async function call(path, { method = "GET", body, key, kv, noKv, vars } = {}) {
    const store = kv ?? fakeKV();
    // 路径补前导斜杠：KV 的键名（`contributors.json`）当 URL 用时漏了斜杠不会报错，
    // 只会静默落到兜底 404——那条断言就永远测不到它想测的分支
    const href = `https://ack.example.com${path.startsWith("/") ? "" : "/"}${path}${key ? `?key=${encodeURIComponent(key)}` : ""}`;
    const request = new Request(href, {
        method,
        body,
        headers: body ? { "content-type": "application/json" } : undefined,
    });
    const env = { ACK: noKv ? undefined : store, SECRET_TOKEN: "s3cret", ...vars };
    const { ctx, pending } = ctxTracker();
    const res = await worker.fetch(request, env, ctx);
    await Promise.all(pending);
    return { res, kv: store };
}

/** 打一次 /skins */
async function skins(names, kv = fakeKV()) {
    const { ctx, pending } = ctxTracker();
    const request = new Request("https://ack.example.com/skins", {
        method: "POST",
        body: JSON.stringify({ names }),
        headers: { "content-type": "application/json" },
    });
    const res = await worker.fetch(request, { ACK: kv, SECRET_TOKEN: "x" }, ctx);
    await Promise.all(pending);
    return { res, kv };
}

let passed = 0;
const check = (label) => {
    passed++;
    console.log(`  ok  ${label}`);
};

/* ---------------- resolve ---------------- */

console.log("resolve");

await withFetch(healthy, async (calls) => {
    const kv = fakeKV();
    const out = await resolve("Player0", { ACK: kv }, null);
    assert.equal(out.uuid, ID);
    assert.equal(out.textures, TEXTURE_URL);
    assert.deepEqual(calls, ["api.mojang.com", "sessionserver.mojang.com"], "冷路径该正好两次外呼");
    const row = JSON.parse(kv.store.get("skin:player0"));
    assert.equal(row.u, ID);
    assert.equal(row.n, "Player0");
    assert.ok(Math.abs(Date.now() - row.t) < 5000, "t 该是本次写入的时刻");
});
check("cold path resolves once and stores the row");

await withFetch(healthy, async (calls) => {
    const kv = fakeKV({ "skin:player0": JSON.stringify({ u: "cached", x: TEXTURE_URL, t: Date.now() }) });
    const out = await resolve("Player0", { ACK: kv }, null);
    assert.equal(out.uuid, "cached");
    assert.equal(calls.length, 0, "命中缓存不该出网");
});
check("warm path makes zero outbound calls");

await withFetch(healthy, async (calls) => {
    const stale = Date.now() - 91 * 24 * 3600 * 1000;
    const kv = fakeKV({ "skin:player0": JSON.stringify({ u: "old", t: stale }) });
    const out = await resolve("Player0", { ACK: kv }, null);
    assert.equal(out.uuid, ID, "改名会把名字交给别人，过期必须重查");
    assert.equal(calls.length, 2);
});
check("stale row is re-resolved");

await withFetch(healthy, async () => {
    const broken = fakeKV({ "skin:player0": "{not json" });
    const out = await resolve("Player0", { ACK: broken }, null);
    assert.equal(out.uuid, ID, "坏档该当作未命中");
});
check("a corrupt row degrades to a miss");

// 这一组最要紧：一次网络抖动被冻成"永久错误的 UUID"比查不到难修一个数量级
for (const [label, make] of [
    ["404", () => empty(404)],
    ["429", () => empty(429)],
    ["5xx", () => empty(500)],
    ["空 body", () => empty(204)],
    ["非法 body", () => new Response("<html>", { status: 200 })],
    ["出网抛异常", async () => { throw new Error("boom"); },],
]) {
    await withFetch([[MOJANG, make]], async () => {
        const kv = fakeKV();
        const out = await resolve("Whoever", { ACK: kv }, null);
        assert.ok(out.error, `${label} 该算失败`);
        assert.equal(out.uuid, undefined, `${label} 不许带 uuid：服务端回写会拿它当真值`);
        assert.equal(kv.store.has("skin:whoever"), false, `${label} 不该落表`);
    });
}
check("no failure shape reaches the table");

await withFetch(healthy, async (calls) => {
    const kv = fakeKV();
    for (const bad of ["", "a b", "中文玩家", "x".repeat(17), "not@valid", "Ban-x"]) {
        assert.deepEqual(await resolve(bad, { ACK: kv }, null), { error: "bad-name" }, `${JSON.stringify(bad)} 该被拒`);
    }
    assert.equal(calls.length, 0, "非法名字一次网都不该出");
});
check("illegal names never reach Mojang");

await withFetch(
    [
        [MOJANG, () => ok({ id: ID, name: "Player0" })],
        [SESSION, () => empty(500)],
    ],
    async () => {
        const kv = fakeKV();
        const out = await resolve("Player0", { ACK: kv }, null);
        assert.equal(out.uuid, ID, "贴图那条腿挂了不该带走 uuid");
        assert.equal(out.textures, undefined);
    }
);
check("uuid survives a failing sessionserver");

// 贴图 URL 那道闸：它会被写进我们自己的表、下次由我们发出去，所以只认 Mojang 的静态 CDN
for (const [label, url, expect] of [
    ["第三方域被拒", "https://cdn.evil.example/x.png", undefined],
    ["明文被拒", "http://textures.minecraft.net/texture/a", undefined],
    ["userinfo 伪装被拒", "https://textures.minecraft.net@evil.example/a", undefined],
    ["大写主机仍通过", "https://TEXTURES.Minecraft.net/texture/a", "https://TEXTURES.Minecraft.net/texture/a"],
]) {
    await withFetch(
        [
            [MOJANG, () => ok({ id: "uuid-t", name: "Player0" })],
            [SESSION, () => ok({ properties: [{ name: "textures", value: textureValue(url) }] })],
        ],
        async () => {
            const out = await resolve("Player0", { ACK: fakeKV() }, null);
            assert.equal(out.uuid, "uuid-t", `${label}：uuid 该照常回`);
            assert.equal(out.textures, expect, label);
        }
    );
}
check("texture url host gate");

await withFetch(
    [
        [MOJANG, () => ok({ id: "uuid-c", name: "Player0" })],
        [SESSION, () => ok({ properties: [{ name: "textures", value: btoa(JSON.stringify({ skins: { classic: { url: "https://textures.minecraft.net/texture/c" } } })) }] })],
    ],
    async () => {
        const out = await resolve("Player0", { ACK: fakeKV() }, null);
        assert.equal(out.textures, "https://textures.minecraft.net/texture/c", "没有 model 档时回落 classic");
    }
);
check("classic skin is the fallback");

/* ---------------- validateList ---------------- */

console.log("validateList");

const GOOD = JSON.stringify({ version: "1", people: [{ name: "Player0", minecraftId: true }] });
for (const raw of [
    GOOD,
    JSON.stringify({ version: "1", people: [] }), // 空名单是合法状态（他还没加人）
    JSON.stringify({ version: "1", people: [{ name: "a", minecraftId: false, avatar: "https://x.test/a.png" }] }),
]) {
    assert.equal(validateList(raw), null, `该放过：${raw}`);
}
for (const [label, raw] of [
    ["不是 JSON", "{oops"],
    ["顶层是数组", "[]"],
    ["缺 version", JSON.stringify({ people: [] })],
    ["version 非字符串", JSON.stringify({ version: 1, people: [] })],
    ["version 空串", JSON.stringify({ version: "", people: [] })],
    ["缺 people", JSON.stringify({ version: "1" })],
    ["people 非数组", JSON.stringify({ version: "1", people: {} })],
    ["行不是对象", JSON.stringify({ version: "1", people: ["a"] })],
    ["空名字", JSON.stringify({ version: "1", people: [{ name: "  ", minecraftId: false }] })],
    ["缺 minecraftId", JSON.stringify({ version: "1", people: [{ name: "a" }] })],
    ["minecraftId 是字符串", JSON.stringify({ version: "1", people: [{ name: "a", minecraftId: "true" }] })],
    ["avatar 是 null", JSON.stringify({ version: "1", people: [{ name: "a", avatar: null, minecraftId: false }] })],
]) {
    assert.notEqual(validateList(raw), null, `该校验出：${label}`);
}
check("shape checks");

/* ---------------- 路由 ---------------- */

console.log("worker.fetch");

assert.equal((await call(LIST_KEY)).res.status, 404);
assert.equal((await call(LIST_KEY)).res.headers.get("x-sideshift-ack"), "not-found");
assert.equal((await call(LIST_KEY, { method: "POST", body: "{}" })).res.status, 405);
check("missing list answers 404 with the marker header");

// 原样透传：不 parse 再 stringify ⇒ 键顺序与缩进都是他提交的那一份；只换占位符
const RAW = '{ "people": [ { "name": "乙", "avatar": "https://{ENDPOINT_HOST}/avatars/b.png" } ], "version": "9" }';
const passed12 = await call(LIST_KEY, { kv: fakeKV({ [LIST_KEY]: RAW }), vars: { ACK_CDN: "ack.example.com" } });
assert.equal(await passed12.res.text(), RAW.replace("{ENDPOINT_HOST}", "ack.example.com"));
assert.notEqual(RAW, JSON.stringify(JSON.parse(RAW)), "样例本身就该不是紧凑格式，否则这条测不出重序列化");
assert.equal(passed12.res.headers.get("access-control-allow-origin"), null, "读口不该开 CORS");
check("list passes through with only the placeholder substituted");

const passed13 = await call(LIST_KEY, { kv: fakeKV({ [LIST_KEY]: RAW }) });
assert.equal(await passed13.res.text(), RAW, "ACK_CDN 没配时占位符该原样留着，由客户端再换一次");
check("placeholder survives when ACK_CDN is absent");

for (const bad of [undefined, "", "nope", "s3cre", "s3cretX"]) {
    for (const method of ["GET", "POST"]) {
        const { res } = await call("/submit", { method, body: method === "POST" ? GOOD : undefined, key: bad });
        assert.equal(res.status, 401, `口令 ${JSON.stringify(bad)} / ${method} 不该通过`);
    }
}
check("submit needs the token, both directions");

const writeKv = fakeKV();
const wrote = await call("/submit", { method: "POST", body: GOOD, key: "s3cret", kv: writeKv });
assert.equal(wrote.res.status, 200);
assert.deepEqual(await wrote.res.json(), { ok: true, version: "1", people: 1 });
assert.equal(writeKv.store.get(LIST_KEY), GOOD, "写入必须原样，不许重新序列化");
const rejected = await call("/submit", { method: "POST", body: "{oops", key: "s3cret", kv: writeKv });
assert.equal(rejected.res.status, 400);
assert.equal(writeKv.store.get(LIST_KEY), GOOD, "校验失败不许覆盖已有名单");
assert.equal((await call("/submit", { method: "GET", key: "s3cret", kv: writeKv })).res.headers.get("cache-control"), "no-store");
check("write path validates before touching KV");

// 这一组必须跑在 withFetch 里：不桩掉 globalThis.fetch 的话，真网络一挂就只能测到"全错"
await withFetch(healthy, async (calls) => {
    const deduped = await skins(["Player0", "Player0", "Player0"]);
    assert.equal(deduped.kv.store.size, 1, "去重失效会写出多份同名档");
    assert.equal(calls.length, 2, "同名三次该只打 Mojang + sessionserver 各一次");

    calls.length = 0;
    const capped = await skins(Array.from({ length: 65 }, (_, i) => `Player${i}`));
    assert.equal(capped.res.status, 400, "批量上限没生效");
    assert.equal(calls.length, 0, "超限不该先打出去再拒绝");

    assert.equal((await skins([])).res.status, 400);
    assert.equal(
        (await skins(["Player0"], deduped.kv)).res.headers.get("cache-control"),
        "public, max-age=300",
        "0 等于关缓存，白烧额度"
    );
});
check("skins dedupes, caps the batch and stays cacheable");

assert.equal((await call("/skins", { method: "OPTIONS" })).res.status, 204);
assert.equal((await call("/submit", { method: "OPTIONS" })).res.status, 404, "带口令的路径不该预检放行");
assert.equal((await call("/nope")).res.status, 404);
check("preflight is scoped to /skins");

assert.equal((await call(LIST_KEY, { noKv: true })).res.status, 404, "没建 KV 绑定时该回未命中而不是抛异常");
check("missing KV binding degrades to 404");

console.log(`\n${passed} 组通过`);

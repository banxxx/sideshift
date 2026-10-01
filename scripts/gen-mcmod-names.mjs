/**
 * 重新生成内置模组名称词典 src-tauri/assets/mcmod-names.tsv
 *
 * 用法：node scripts/gen-mcmod-names.mjs <mcmod.buf 路径>
 * 输入是 MC百科快照的 protobuf 列表（字段 1=WikiId 2=中文名 3=CF slug 4=Modrinth slug），
 * 输出只留「有中文名」那 9,500 行，按 WikiId 升序（重名行必须全留：改写那一侧要靠它们判唯一）。
 *
 * 词典随 exe 内置、不参与端判定与方案存档，所以重新生成是**发版期人工动作**（无 TTL）。
 * 出处与授权的那份说明放在本机 `.scratch/mcmod-names.NOTICE.md`（不进仓库），仓库里要留的只有
 * 下面写进表头的两行：出处 + 署名落点（应用「关于 › 许可与依赖」那一行）。
 */
import { readFileSync, writeFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import path from "node:path";

const src = process.argv[2];
if (!src) {
    console.error("用法：node scripts/gen-mcmod-names.mjs <mcmod.buf 路径>");
    process.exit(1);
}

function readVarint(buf, pos) {
    let shift = 0n;
    let value = 0n;
    for (;;) {
        const byte = buf[pos.i++];
        value |= BigInt(byte & 0x7f) << shift;
        if ((byte & 0x80) === 0) return value;
        shift += 7n;
    }
}

/** protobuf wire 格式逐字段产出：0=varint，2=length-delimited（其余编码这份数据里没有） */
function* fields(buf, from, to) {
    const p = { i: from };
    while (p.i < to) {
        const key = readVarint(buf, p);
        const no = Number(key >> 3n);
        const wt = Number(key & 7n);
        if (wt === 0) yield [no, readVarint(buf, p), null];
        else if (wt === 2) {
            const len = Number(readVarint(buf, p));
            yield [no, null, buf.subarray(p.i, p.i + len)];
            p.i += len;
        } else throw new Error(`不支持的 wire type ${wt}（字段 ${no}）`);
    }
}

const raw = gunzipSync(readFileSync(src));
const rows = [];
for (const [no, , bytes] of fields(raw, 0, raw.length)) {
    if (no !== 1 || !bytes) throw new Error("顶层不是 repeated 消息，快照格式变了");
    const rec = { wikiId: 0, name: "", cf: "", mr: "" };
    for (const [f, v, s] of fields(bytes, 0, bytes.length)) {
        if (f === 1) rec.wikiId = Number(v);
        else if (f === 2) rec.name = s?.toString("utf8").trim() ?? "";
        else if (f === 3) rec.cf = s?.toString("utf8").trim().toLowerCase() ?? "";
        else if (f === 4) rec.mr = s?.toString("utf8").trim().toLowerCase() ?? "";
    }
    if (rec.name) rows.push(rec);
}

// 一个 (中文名, slug) 一行，两侧同 slug 的自然合成一条：同一中文名下挂多个不同 slug 的组很多
// （实测 CF 侧 378 组、Modrinth 侧 37 组，如「铁路 (Railcraft)」同时有 railcraft 与 railcraft-reborn），
// 按词条归并会把后一个 slug 静默丢掉，那一侧的模组就查不到中文名了。
// 不记来源侧：显示名反查与来源无关，改写词优先取中文名括号里的英文原名（9,500 行里 9,389 行都带），
// 完全没有 slug 的词条也留一行（slug 空）：它的括号英文仍能当改写词用
const pairs = new Map();
for (const r of rows.sort((a, b) => a.wikiId - b.wikiId)) {
    const slugs = [r.cf, r.mr].filter(Boolean);
    if (!slugs.length) {
        if (!pairs.has(`${r.name}\u0000`)) pairs.set(`${r.name}\u0000`, { name: r.name, slug: "" });
        continue;
    }
    for (const slug of slugs) {
        const key = `${r.name}\u0000${slug}`;
        if (!pairs.has(key)) pairs.set(key, { name: r.name, slug });
    }
}
const out = [...pairs.values()];

const header = [
    "# 模组名称词典：MC百科词条中文名 ↔ 平台 slug（内置进 exe，不参与端判定与方案存档）",
    "# 出处：MC百科词条中文名快照（CC BY-NC-SA 4.0，署名在应用「关于 › 许可与依赖」）；重新生成：node scripts/gen-mcmod-names.mjs <快照>",
    `# 列：中文名<TAB>slug（空 slug=该词条没对齐任何平台，只用于中文词改写）；#开头为注释行`,
    `# 行数：${out.length}（生成于 ${new Date().toISOString().slice(0, 10)}）`,
].join("\n");
const body = out.map((r) => [r.name, r.slug].join("\t")).join("\n");
const dest = path.resolve(import.meta.dirname, "../src-tauri/assets/mcmod-names.tsv");
writeFileSync(dest, `${header}\n${body}\n`, "utf8");

console.log(dest);
console.log(
    `行 ${out.length}（不同中文名 ${new Set(out.map((r) => r.name)).size} / ` +
        `空 slug ${out.filter((r) => !r.slug).length} / ` +
        `一名多 slug 的中文名 ${
            out.length - new Set(out.map((r) => r.name)).size
        }）`
);

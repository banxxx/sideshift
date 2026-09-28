/**
 * i18n 的自检与派生文件生成（语义键模型）。
 *
 *   node scripts/i18n.mjs check        覆盖率 / 孤儿 / 源文漂移 / 插值槽 / 裸键（改完跑这个）
 *   node scripts/i18n.mjs sync         重生成 source-lock.json 与 source-keys.ts（新增动态句后跑）
 *   node scripts/i18n.mjs sync-tw      按内联中文补 zh-TW 目录里缺的条目
 *   node scripts/i18n.mjs export [csv] 出翻译工单：键 / 中文原文 / 两种译文 / 状态
 *   node scripts/i18n.mjs keys [过滤]  列出租件（查漏网）
 *
 * ## 键 = 语义名，中文原文留在调用点
 *
 * `t("settings.interface-language", "界面语言")`：第一个参数是稳定标识，第二个是**简体中文原文**，
 * 由 i18next 当 `defaultValue` 用。三个后果都是故意的：
 *
 *  1. **zh-CN 仍然没有目录文件**：中文档查不到键 ⇒ 返回内联原文，中文界面因此结构上不可能错翻。
 *     （实测见探针：`t(key, 原文, args)` 在三档下分别得原文／繁体／英文，缺译一律露原文而非裸键。）
 *  2. 键与文案脱钩 ⇒ 改中文不用重命名键，英文/繁体的译文条目也不会被顺手丢掉；
 *     同一句中文要两种译法时，两处给两个键就行（原文即键那条路的死穴）。
 *  3. 代价是**中文改了不会自动惊动译文**，所以有 `source-lock.json`：它记住每个键上次对应的中文，
 *     内联原文与它不符 ⇒ `STALE`，那条键的译文进「待重审」。这一条闸门是整套方案的承重墙，
 *     没有它就退化成「语义键 + 静默错译」，比原文即键更差。
 *
 * ## 动态句（运行时才知道内容的中文）
 *
 * 后端发来的整句、存进 store 的原文、下拉的 option.label —— 这些没有字面量可留在调用点，
 * 走 `source-keys.ts` 那张「中文原文 → 语义键」表（本脚本生成，别手改）。
 * 表里有的查目录，没有的原样返回中文（行为与接入前一致）。
 * 新加一句：在显示处写 `/*i18n:键=那句中文*\/`（已有键可省键名），再跑 `sync`。
 */
import { readFileSync, writeFileSync, readdirSync, statSync, existsSync, mkdirSync } from "node:fs";
import { join, relative } from "node:path";
import { createRequire } from "node:module";

const ROOT = process.cwd();
const RES = join(ROOT, "src/lib/i18n/resources");
const LOCK = join(RES, "source-lock.json");
const SOURCE_KEYS = join(ROOT, "src/lib/i18n/source-keys.ts");
const TARGETS = ["en-US", "zh-TW"];
/** 繁体目录里只写「与原文不同」的条目：相同的查不到会回落到内联原文，那正是正确答案 */
const TW_SKIP_IDENTICAL = true;

const CJK = /[㐀-鿿]/;
const require = createRequire(join(ROOT, "package.json"));

/**
 * `t("键", "原文")`：前两参都必须是字面量，第三参（`{ count }` 那类）不看内容。
 * 一参形态（`t("键")`）也算命中但原文为空 ⇒ check 报 NEEDS-ZH：语义键模型里原文不在调用点，
 * zh-CN 那一档就会露出裸键。
 */
const T_CALL =
    /(?:^|[^A-Za-z0-9_$.])(?:t|\$t)\(\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')\s*(?:,\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`))?/g;
const BACKEND_MACRO = /\bmsg!\(\s*"((?:[^"\\]|\\.)*)"/g;
/**
 * 动态句声明：`/*i18n:键=那句中文*\/`，或省略键名的 `/*i18n:那句中文*\/`
 * （省略时按「这句中文在别处有字面量调用」从锁里取键；两处都没有就必须写键名）。
 * 一行一句，块标记用于后端那三十几条固定句。
 */
const KEY_MARK = /\/\*i18n[:：]\s*([^*]+?)\s*\*\//g;

function* walk(dir, ext = /\.(ts|tsx)$/) {
    for (const name of readdirSync(dir)) {
        if (name === "node_modules" || name === "target" || name === "dist" || name === ".git") continue;
        const p = join(dir, name);
        const st = statSync(p);
        // 分隔符统一成 `/`：Windows 下 join 给的是反斜杠，后面所有 "includes('/i18n/')" 式过滤都会落空
        if (st.isDirectory()) yield* walk(p, ext);
        else if (ext.test(name)) yield p.split("\\").join("/");
    }
}

/** 注释一律剥掉：这个仓库的注释密度极高，不剥会把设计说明当待翻文案（块标记单独扫原文） */
function stripComments(src) {
    return src
        .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
        .replace(/(^|[^:\\])\/\/[^\n]*/g, "$1");
}

const unquote = (raw) => raw.slice(1, -1).replace(/\\(["'`\\])/g, "$1");
const slots = (s) => [...s.matchAll(/\{\{\s*([A-Za-z0-9_]+)/g)].map((m) => m[1]).sort();
const rel = (p) => relative(ROOT, p).split("\\").join("/");
const sortedJson = (o) =>
    JSON.stringify(Object.fromEntries(Object.entries(o).sort((a, b) => (a[0] < b[0] ? -1 : 1))), null, 2) + "\n";

/**
 * 扫源码，产出两样东西：
 *  - `statics`: 键 -> { zh, files }（调用点带内联原文的那些）
 *  - `dynamics`: 中文原文 -> { key?, files }（标记块 + 后端 msg! 模板，运行时才知道内容）
 */
function collect() {
    const statics = new Map();
    const dynamics = new Map();
    const note = (map, k, file, extra) => {
        const hit = map.get(k) ?? { files: new Set(), ...extra };
        hit.files.add(rel(file));
        map.set(k, hit);
    };
    for (const file of walk(join(ROOT, "src"))) {
        if (file.includes("/i18n/") || file.includes("mock")) continue;
        const raw = readFileSync(file, "utf8");
        // 1) 动态句声明：活在注释里，必须读原文并逐行拆（排版缩进不是句子的一部分）
        for (const m of raw.matchAll(KEY_MARK)) {
            for (const line of m[1].split("\n")) {
                const row = line.trim();
                if (!row) continue;
                const [key, zh] = /^([\w.\-]+)\s*[=＝]\s*(.+)$/.exec(row)?.slice(1) ?? [];
                const text = (zh ?? row).trim();
                if (!CJK.test(text)) continue;
                const hit = dynamics.get(text) ?? { files: new Set(), key: undefined };
                if (key) hit.key = key;
                hit.files.add(rel(file));
                dynamics.set(text, hit);
            }
        }
        // 2) 静态调用点：`t(键, 原文)`，注释剥过之后才扫
        const src = stripComments(raw);
        for (const m of src.matchAll(T_CALL)) {
            const key = unquote(m[1]);
            if (!/^[a-z0-9][\w.\-]*$/.test(key)) continue;
            const zh = m[2] === undefined ? "" : unquote(m[2]);
            const hit = statics.get(key) ?? { zh: undefined, files: new Set() };
            if (hit.zh !== undefined && zh && hit.zh !== zh) hit.conflict = true;
            if (zh) hit.zh = zh;
            hit.files.add(rel(file));
            statics.set(key, hit);
        }
    }
    const rsDir = join(ROOT, "src-tauri/src");
    if (existsSync(rsDir)) {
        for (const file of walk(rsDir, /\.rs$/)) {
            let src = stripComments(readFileSync(file, "utf8"));
            // 测试模块里的 msg! 只是渲染断言，不是往外发的句子：算进去会凭空多出键
            src = src.split("#[cfg(test)]")[0];
            for (const m of src.matchAll(BACKEND_MACRO)) note(dynamics, m[1], file, { key: undefined });
        }
    }
    return { statics, dynamics };
}

function readLock() {
    if (!existsSync(LOCK)) return {};
    return JSON.parse(readFileSync(LOCK, "utf8"));
}

function readLocale(lng) {
    const table = {};
    const dupes = [];
    const dir = join(RES, lng);
    const shards = existsSync(dir) ? readdirSync(dir).filter((n) => n.endsWith(".json")).sort() : [];
    for (const shard of shards) {
        const json = JSON.parse(readFileSync(join(dir, shard), "utf8"));
        for (const [k, v] of Object.entries(json)) {
            if (k in table && table[k] !== v) dupes.push([k, shard]);
            table[k] = v;
        }
    }
    return { table, dupes, shards };
}

/** 键的前缀就是分片名（= 谁 owns 这条），所以写目录不必再猜归属 */
const shardOf = (key) => key.split(".")[0];

/** 一个键在这份表里算不算「有译文」：count 键看 _one/_other，其余看基础形 */
const hasEntry = (table, key, zh) =>
    key in table || (/\{\{\s*count\s*\}\}/.test(zh ?? "") && (`${key}_one` in table || `${key}_other` in table));

/** 表里属于这个键的所有条目（含单复数两形），用来逐条比对插值槽 */
function entriesOf(table, key) {
    return Object.keys(table)
        .filter((k) => k === key || (k.startsWith(`${key}_`) && /^_one$|^_other$/.test(k.slice(key.length))))
        .map((k) => [k, table[k]]);
}

function allKeys({ statics, dynamics }, lock) {
    // 全集 = 静态调用点的键 ∪ 动态句解析出的键 ∪ 锁里的键（锁里多出来的是孤儿候选）
    const set = new Map(); // key -> zh
    for (const [key, meta] of statics) if (meta.zh) set.set(key, meta.zh);
    for (const [zh, meta] of dynamics) {
        const key = meta.key ?? keyOfZh(zh, statics, lock);
        if (key) set.set(key, zh);
    }
    return set;
}
/** 动态句没写键名时：这句话在别处有字面量调用就复用那个键，否则查锁（都查不到 = UNKEYED） */
function keyOfZh(zh, statics, lock) {
    for (const [key, meta] of statics) if (meta.zh === zh) return key;
    return Object.keys(lock).find((k) => lock[k] === zh);
}

function check({ fix = false } = {}) {
    const collected = collect();
    const { statics, dynamics } = collected;
    const lock = readLock();
    const keys = allKeys(collected, lock);
    let problems = 0;
    const say = (tag, msg) => console.log(`${tag} ${msg}`);

    const perLocale = {};
    for (const lng of TARGETS) perLocale[lng] = readLocale(lng);

    // 1) 键必须有内联原文
    for (const [key, meta] of statics)
        if (!meta.zh) {
            problems++;
            say("NEEDS-ZH", `${key} 的调用点没带中文原文（zh-CN 会露裸键）← ${[...meta.files][0]}`);
        }
    // 2) 动态句必须解析到键
    const unkeyed = [...dynamics.entries()].filter(
        ([zh, meta]) => !(meta.key ?? keyOfZh(zh, statics, lock))
    );
    for (const [zh, meta] of unkeyed) {
        problems++;
        say("UNKEYED", `动态句没有键：${zh.slice(0, 50)} ← ${[...meta.files][0]}`);
    }
    // 3) 漂移闸门：锁里的中文与调用点的中文不符 ⇒ 源文改过，译文待重审
    const stale = [];
    for (const [key, meta] of statics)
        if (meta.zh && key in lock && lock[key] !== meta.zh) stale.push([key, lock[key], meta.zh]);
    for (const [key, was, now] of stale) {
        problems++;
        say("STALE", `${key} 源文已改（译文待重审）\n      旧: ${was}\n      新: ${now}`);
    }
    for (const [key, meta] of statics)
        if (meta.conflict) {
            problems++;
            say("CONFLICT", `同一个键在调用点带了两份中文：${key} ← ${[...meta.files].slice(0, 3).join(" ")}`);
        }

    // 4) 缺译 + 5) 插值槽 + 6) 单复数
    const missing = {};
    for (const lng of TARGETS) {
        missing[lng] = [];
        const table = perLocale[lng].table;
        for (const [key, zh] of keys) {
            if (!hasEntry(table, key, zh)) {
                if (lng === "zh-TW" && TW_SKIP_IDENTICAL && twOf(zh) === zh) continue;
                missing[lng].push(key);
                continue;
            }
            for (const [entry, value] of entriesOf(table, key)) {
                const a = slots(zh).join(",");
                const b = slots(value).join(",");
                if (a !== b) {
                    problems++;
                    say("SLOT", `${lng} ${entry} 插值槽与原文不一致\n      原文: ${zh}\n      译文: ${value}`);
                }
            }
            if (/\{\{\s*count\s*\}\}/.test(zh) && lng === "en-US" && !(`${key}_one` in table))
                say("PLURAL", `${lng} ${key} 是 count 键却没有 _one/_other`);
        }
    }

    // 7) 孤儿：目录里有、源码不再用
    for (const lng of TARGETS) {
        // `_one`/`_other` 是 i18next 自己拼的后缀，源码里不会出现带后缀的键 ⇒ 先剥回基础形再判
        const orphans = Object.keys(perLocale[lng].table).filter(
            (k) =>
                !keys.has(k) && !(/_one$|_other$/.test(k) && keys.has(k.replace(/_one$|_other$/, "")))
        );
        if (orphans.length) {
            problems++;
            say("ORPHAN", `${lng} 有 ${orphans.length} 条源码不再使用的键`);
            orphans.slice(0, 12).forEach((k) => console.log(`      ${k}`));
        }
        for (const [k, shard] of perLocale[lng].dupes) {
            problems++;
            say("DUP", `${lng} 同一键两片两译：${k} ← ${shard}`);
        }
        // 8) 裸键进目录：键名长得像键的译文（改键名时的残留）
        const leak = Object.entries(perLocale[lng].table).filter(
            ([, v]) => lng === "en-US" && CJK.test(v)
        );
        if (leak.length) {
            problems++;
            say("LEAK", `en-US 有 ${leak.length} 条译文仍含汉字`);
            leak.slice(0, 12).forEach(([k, v]) => console.log(`      ${k} → ${v}`));
        }
    }

    // 9) 中文残留（只扫会进界面语法位置的那些）
    //    逐行扫：JSX 文本位的正则不排除换行时 `>` 会一路吃到下一段的 `<`，把已翻好的句子报成残留。
    const residue = [];
    const UI_ATTR =
        /\b(?:title|label|placeholder|desc|sub|description|emptyText|errorText|tip|text|children)\s*=\s*\{?\s*["'`][^"'`\n]*[㐀-鿿][^"'`\n]*["'`]/g;
    const JSX_TEXT = />([^<>{}\n]*[㐀-鿿][^<>{}\n]*)</g;
    for (const file of walk(join(ROOT, "src"))) {
        if (file.includes("i18n") || file.includes("mock")) continue;
        const lines = stripComments(readFileSync(file, "utf8")).split("\n");
        lines.forEach((row, i) => {
            for (const m of row.matchAll(JSX_TEXT)) push(m[1]);
            for (const m of row.matchAll(UI_ATTR)) push(/["'`]([^"'`\n]*)["'`]/.exec(m[0])?.[1] ?? m[0]);
            function push(inner) {
                if (!CJK.test(inner ?? "")) return;
                residue.push(`${rel(file)}:${i + 1}: ${row.trim().slice(0, 100)}`);
            }
        });
    }

    for (const lng of TARGETS) {
        const total = keys.size;
        const got = total - missing[lng].length;
        say("COVER", `${lng} ${got}/${total} (${((got / total) * 100).toFixed(1)}%)`);
    }
    for (const lng of TARGETS)
        if (missing[lng].length) {
            console.log(`\n${lng} 缺 ${missing[lng].length} 条：`);
            const byShard = new Map();
            for (const k of missing[lng]) (byShard.get(shardOf(k)) ?? byShard.set(shardOf(k), []).get(shardOf(k))).push(k);
            for (const [s, list] of [...byShard].sort()) {
                console.log(`  [${s}.json] ${list.length} 条`);
                list.forEach((k) => console.log(`    ${k}\t${keys.get(k)}`));
            }
            if (lng === "zh-TW" && fix) continue;
            problems++;
        }
    if (residue.length) {
        console.log(`\n中文残留 ${residue.length} 处（候选，需人工确认）：`);
        residue.slice(0, 80).forEach((r) => console.log(`  ${r}`));
        problems++;
    }
    console.log(problems ? `\ncheck: ${problems} 类问题` : "\ncheck: 通过");
    return { collected, keys, missing, problems };
}

/**
 * 繁体的「词」不只靠字形转换：OpenCC 只做 cn→t 的字级 + 少量词组映射，
 * 术语要按台湾 UI 习惯另换一遍（设置→設定、构建→建置…），否则繁体界面读起来是
 * 「用繁体字写的简体文案」。
 */
const TW_TERMS = [
    ["服务器", "伺服器"],
    ["服务端", "伺服器端"],
    ["用户", "使用者"],
    ["文件夹", "資料夾"],
    ["文件名", "檔案名稱"],
    ["文件", "檔案"],
    ["默认", "預設"],
    ["设置", "設定"],
    ["构建", "建置"],
    ["加载", "載入"],
    ["链接", "連結"],
    ["网络", "網路"],
    ["缓存", "快取"],
    ["内存", "記憶體"],
    ["线程", "執行緒"],
    ["进程", "處理程序"],
    ["数据", "資料"],
    ["磁盘", "磁碟"],
    ["端口", "連接埠"],
    ["超时", "逾時"],
    ["导出", "匯出"],
    ["导入", "匯入"],
    ["图标", "圖示"],
    ["界面", "介面"],
    ["软件", "軟體"],
    ["硬件", "硬體"],
    ["搜索", "搜尋"],
    ["支持", "支援"],
    ["质量", "品質"],
    ["反馈", "回饋"],
    ["登录", "登入"],
    ["注销", "登出"],
    ["账号", "帳號"],
    ["账户", "帳戶"],
    ["窗口", "視窗"],
    ["菜单", "選單"],
    ["工具栏", "工具列"],
    ["状态栏", "狀態列"],
    ["鼠标", "滑鼠"],
    ["屏幕", "螢幕"],
    ["字节", "位元組"],
    ["项目", "專案"],
    ["响应", "回應"],
];
/** OpenCC 会把「平台」写成「臺」——台湾正字里 臺灣 用 臺，但 平台/丽萍 一类仍作 平台 */
const TW_AFTER = [["平臺", "平台"]];
let conv = null;

function toTraditional(s) {
    let out = s;
    for (const [from, to] of TW_TERMS) out = out.split(from).join(to);
    out = conv(out);
    for (const [from, to] of TW_AFTER) out = out.split(from).join(to);
    return out;
}

/** 懒加载：check 也问一句「这条繁简同形吗」，而 opencc 那份词典不该白读 */
function twOf(key) {
    conv ??= require("opencc-js").Converter({ from: "cn", to: "tw" });
    return toTraditional(key);
}

function syncTw() {
    conv = require("opencc-js").Converter({ from: "cn", to: "tw" });
    const { keys } = check({ fix: true });
    const existing = readLocale("zh-TW").table;
    const buckets = new Map();
    let added = 0;
    for (const [key, zh] of keys) {
        if (key in existing) continue;
        const tw = twOf(zh);
        if (TW_SKIP_IDENTICAL && tw === zh) continue;
        (buckets.get(shardOf(key)) ?? buckets.set(shardOf(key), {}).get(shardOf(key)))[key] = tw;
        added++;
    }
    for (const [shard, entries] of buckets) {
        const p = join(RES, "zh-TW", `${shard}.json`);
        const merged = existsSync(p) ? JSON.parse(readFileSync(p, "utf8")) : {};
        if (!existsSync(join(RES, "zh-TW"))) mkdirSync(join(RES, "zh-TW"), { recursive: true });
        writeFileSync(p, sortedJson({ ...merged, ...entries }), "utf8");
    }
    console.log(`\nsync-tw: 补了 ${added} 条（其余与简体同形，靠回落）`);
}

/**
 * 重生成两份派生文件：
 *  - `source-lock.json`：键 -> 中文原文（漂移基线，也是翻译工单的源数据）
 *  - `source-keys.ts`：中文原文 -> 键，只收录**动态句**（运行时才知道内容的那些）
 */
function sync() {
    const collected = collect();
    const { statics, dynamics } = collected;
    const lock = readLock();
    const next = { ...lock };
    for (const [key, meta] of statics) if (meta.zh) next[key] = meta.zh;
    for (const [zh, meta] of dynamics) {
        const key = meta.key ?? keyOfZh(zh, statics, lock);
        if (!key) {
            console.log(`UNKEYED 动态句还没有键名，补上 /*i18n:键=…*/ 再 sync：${zh.slice(0, 60)}`);
            continue;
        }
        next[key] ??= zh;
    }
    writeFileSync(LOCK, sortedJson(next), "utf8");

    const table = {};
    for (const [zh, meta] of dynamics) {
        const key = meta.key ?? keyOfZh(zh, statics, lock);
        if (key) table[zh] = key;
    }
    const body = Object.entries(table)
        .sort((a, b) => (a[1] < b[1] ? -1 : 1))
        .map(([zh, key]) => `    ${JSON.stringify(zh)}: ${JSON.stringify(key)}`)
        .join(",\n");
    writeFileSync(
        SOURCE_KEYS,
        `/**
 * 中文原文 → 语义键。由 \`node scripts/i18n.mjs sync\` 生成，**不要手改**（手改会被下次 sync 抹掉）。
 *
 * 只收录**动态句**：后端发来的整句、store 里的原文、下拉的 option.label —— 那些调用点拿不到字面量、
 * 没法把中文当 \`defaultValue\` 传的地方。静态调用点的原文就在代码里，不进这张表。
 *
 * 表里没有的句子按原样返回中文（行为与未接入目录一致），不是错误；要让某句能翻，
 * 在显示处加一行块注释 \`i18n:键=那句中文\`（写法见 scripts/i18n.mjs 文件头），再跑 sync。
 */
export const SOURCE_KEYS: Readonly<Record<string, string>> = {
${body},
};

export default SOURCE_KEYS;
`,
        "utf8"
    );
    console.log(
        `\nsync: 锁 ${Object.keys(next).length} 条（键→中文），动态句表 ${Object.keys(table).length} 条（中文→键）`
    );
}

/** 翻译工单：一行一条，外部译者只需填译文两列；状态列指出该不该动 */
function exportWorkbench(format = "tsv") {
    const collected = collect();
    const lock = readLock();
    const keys = allKeys(collected, lock);
    const tables = Object.fromEntries(TARGETS.map((l) => [l, readLocale(l).table]));
    const rows = [...keys.entries()]
        .sort((a, b) => (a[0] < b[0] ? -1 : 1))
        .map(([key, zh]) => {
            const en = tables["en-US"][key] ?? tables["en-US"][`${key}_one`] ?? "";
            const tw = tables["zh-TW"][key] ?? "";
            const status = !en ? "MISSING" : lock[key] && lock[key] !== zh ? "STALE" : tw || twOf(zh) === zh ? "OK" : "TW-MISSING";
            return [key, zh, en, tw, status];
        });
    const head = ["key", "zh-CN(源文)", "en-US", "zh-TW", "状态"];
    const text =
        format === "csv"
            ? [head, ...rows].map((r) => r.map((c) => `"${String(c).replace(/"/g, '""')}"`).join(",")).join("\r\n")
            : [head, ...rows].map((r) => r.join("\t")).join("\n");
    const file = join(ROOT, ".scratch", `i18n-workbench.${format}`);
    writeFileSync(file, text + "\n", "utf8");
    const n = (s) => rows.filter((r) => r[4] === s).length;
    console.log(`export: ${rows.length} 行 → ${rel(file)}\n  OK ${n("OK")} / MISSING ${n("MISSING")} / STALE ${n("STALE")} / TW-MISSING ${n("TW-MISSING")}`);
}

/** 广谱残留：把 src 里所有带汉字的字符串字面量捞出来（比 check 的 residue 宽得多）。只报不判错 */
function bareLiterals() {
    const hits = [];
    for (const file of walk(join(ROOT, "src"))) {
        if (file.includes("/i18n/") || file.includes("mock")) continue;
        const raw = readFileSync(file, "utf8");
        const lines = raw.split("\n");
        const src = stripComments(raw);
        for (const m of src.matchAll(/(["'`])((?:[^"'`\n\\]|\\.)*?)\1/g)) {
            if (!CJK.test(m[2])) continue;
            const start = m.index ?? 0;
            const line = src.slice(0, start).split("\n").length;
            const row = lines[line - 1] ?? "";
            // 已经带原文的 t(键, "中文")、动态句标记、锁里的值都不算残留
            const before = src.slice(Math.max(0, start - 80), start);
            const followedByColon = /^\s*:/.test(src.slice(start + m[0].length, start + m[0].length + 2));
            if (/,\s*$/.test(before) || /i18n[:：]/.test(row) || followedByColon) continue;
            hits.push(`${rel(file)}:${line}: ${m[2].trim()}`);
        }
    }
    return hits;
}

const cmd = process.argv[2] ?? "check";
if (cmd === "check") process.exit(check().problems ? 1 : 0);
if (cmd === "sync") sync();
if (cmd === "sync-tw") syncTw();
if (cmd === "export") exportWorkbench(process.argv[3] ?? "tsv");
if (cmd === "bare") {
    const hits = bareLiterals();
    hits.forEach((h) => console.log(h));
    console.log(`\nbare: ${hits.length} 条含汉字字面量（数据值属正常，逐条人工确认）`);
}
if (cmd === "keys") {
    const filter = process.argv[3];
    const collected = collect();
    for (const [key, meta] of collected.statics)
        if (!filter || key.includes(filter) || (meta.zh ?? "").includes(filter))
            console.log(`${key}\t${meta.zh}\t${[...meta.files].join(" ")}`);
}
if (cmd === "count") console.log(`static keys: ${collect().statics.size}`);

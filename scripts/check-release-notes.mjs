#!/usr/bin/env node
/**
 * 发版短日志的形状闸门：`release-notes/<版本>.md` 会被当成 GitHub Release 正文，客户端弹窗按
 * 纯文本 + 4 行 clamp 显示它 ⇒ 排版写错的后果不是报错，而是话被无声截断。
 * 宽度按 CJK=1 格、拉丁=0.55 格估：内容宽 418px、12px 字号 ⇒ 一行约 34 个汉字。
 * 用法：node scripts/check-release-notes.mjs [版本号]（缺省读 package.json 的 version）
 */
import { readFileSync, existsSync } from "node:fs";

// 想放宽/收紧就改这四个数：它们是弹窗那几行的真实预算，不是风格偏好
const UNITS_PER_LINE = 34; // 一个视觉行装得下的 CJK 格数
const MAX_VISUAL_LINES = 4; // line-clamp-4：第 5 行永远看不到
const NEAR_LIMIT = 4; // 距上限不足这么多格就提示（估算有误差）
const LATIN_WEIGHT = 0.55; // Inter 里拉丁字母/数字的平均字宽 / 字号

const WIDE = [0x1100, 0x206f, 0x2e80, 0x303f, 0x3400, 0x4dbf, 0x4e00, 0x9fff, 0xac00, 0xd7a3, 0xf900, 0xfaff, 0xfe30, 0xfe6f, 0xff00, 0xff60, 0xffe0, 0xffe6];

const units = (line) =>
    [...line].reduce((sum, ch) => {
        const c = ch.codePointAt(0);
        const wide = WIDE.some((lo, i) => i % 2 === 0 && c >= lo && c <= WIDE[i + 1]);
        return sum + (wide ? 1 : LATIN_WEIGHT);
    }, 0);

const tag = (s) => (process.env.GITHUB_ACTIONS ? `::${s}::` : `${s.toUpperCase()}: `);
const notes = [];
const fail = (msg) => notes.push(["error", msg]);
const warn = (msg) => notes.push(["warning", msg]);

const version = process.argv[2] ?? JSON.parse(readFileSync("package.json", "utf8")).version;
const file = `release-notes/${version}.md`;

if (!existsSync(file)) {
    console.error(`${tag("error")}缺这一版的更新日志：${file}（应用内弹窗只显示这一个文件的内容）`);
    process.exit(1);
}

const raw = readFileSync(file, "utf8").replace(/\r\n?/g, "\n");
const trimmed = raw.trim();
if (!trimmed) {
    console.error(`${tag("error")}${file} 是空的：它现在就是 Release 正文，不许留空`);
    process.exit(1);
}
if (!raw.endsWith("\n")) warn(`${file} 结尾缺一个换行`);

// 空行与 markdown 结构：弹窗按纯文本渲染，这些既占预算又读不通
const lines = trimmed.split("\n");
let visual = 0;
lines.forEach((line, i) => {
    const no = `第 ${i + 1} 行`;
    if (!line.trim()) return fail(`${no} 是空行 · 会被显示成一行 18px 的空白，删掉它`);
    if (/^---?\s*$/.test(line) || /^#{1,6}\s/.test(line) || /^```/.test(line) || /<!--/.test(line))
        return fail(`${no} 是 markdown/注释结构 · 弹窗不渲染它，符号会原样出现在用户眼前`);
    if (!line.startsWith("- ")) warn(`${no} 建议以「- 」开头，和其余条目排成同一形状`);
    const u = units(line);
    const rows = Math.ceil(u / UNITS_PER_LINE);
    visual += rows;
    if (rows > 1) warn(`${no} 约 ${u.toFixed(1)} 格 · 在弹窗里要绕成 ${rows} 行，尽量压到一行`);
    else if (u > UNITS_PER_LINE - NEAR_LIMIT) warn(`${no} 距单行上限不足 ${NEAR_LIMIT} 格，字号或 DPI 一变就绕`);
});

if (visual > MAX_VISUAL_LINES)
    fail(`全文合计约 ${visual} 个视觉行 · 弹窗只显示前 ${MAX_VISUAL_LINES} 行，多出来的会被截断（完整记录写进 docs/guide/changelog.md）`);
else if (visual === MAX_VISUAL_LINES)
    // 文件必须以一个换行收尾（POSIX），而那个换行会不会被演成第 5 行取决于 body 字符串是否被修剪——这里不赌，只提示
    warn(`已经顶满 ${MAX_VISUAL_LINES} 行 · 文件尾的换行若被原样显示就是多出的一行，建议压到 3 条`);

for (const [kind, msg] of notes) (kind === "error" ? console.error : console.warn)(tag(kind) + msg);
if (notes.some(([k]) => k === "error")) process.exit(1);
console.log(`更新日志形状合格：${file} · ${lines.length} 条约 ${visual} 视觉行（上限 ${MAX_VISUAL_LINES}）`);

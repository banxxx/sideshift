/**
 * 失败信息怎么「说给人听」，全站只这一处：后端只出种类代码（`net:种类:主机` 或 `app:种类`），本文件把代码换成本地化的一句话；
 * URL、路径、状态码一律不上界面，要留着排查的原句走 `TaskError.detail`（「复制诊断信息」读它，不读代码）。
 * `errOf` 对认不出的字符串退回 `tSource`——后端写死的中文整句本就在 source-keys 表里，照常翻。
 */
import { t, tSource } from "./i18n";

/** 主机名 → 界面上叫谁。后缀匹配（镜像与主站同一个名字）。
 *  名字一律以拉丁字母或数字收尾：模板里 `{{site}}` 后面跟着一个空格，这样中英夹排才不会有怪空格 */
const SITES: [string, string][] = [
    ["modrinth.com", "Modrinth"],
    ["modrinth.cn", "Modrinth"],
    ["curseforge.com", "CurseForge"],
    ["minekuai.cn", "麦块 API"],
    ["bangbang93.com", "BMCLAPI"],
    // 查 Modrinth 与 CurseForge 都走它，所以不能叫成任何一家的名字
    ["mcimirror.top", "mcimirror"],
    ["mojang.com", "Mojang"],
    ["minecraft.net", "Mojang"],
    ["forge.gg", "Forge"],
    ["minecraftforge.net", "Forge"],
    ["neoforged.net", "NeoForge"],
    ["fabricmc.net", "Fabric"],
    ["quiltmc.org", "Quilt"],
    ["github.com", "GitHub"],
    ["githubusercontent.com", "GitHub"],
    ["optifine.net", "OptiFine"],
];

/** 认不出来的主机名直接给出去：`example.com 响应太慢` 比「该站点」更有用，也不比它更难读 */
export function siteName(host: string): string {
    const h = host.toLowerCase();
    for (const [suffix, name] of SITES) {
        if (h === suffix || h.endsWith("." + suffix) || h === suffix.split(".")[0]) return name;
    }
    return host || "unknown";
}

/** `net:种类:主机[:状态码]` → 界面那句话；不是这个形状的回 null（交回调用方走 tSource） */
export function netText(code: string): string | null {
    if (!code.startsWith("net:")) return null;
    const [, kind, host, status] = code.split(":");
    const site = host ? siteName(host) : siteName("unknown");
    switch (kind) {
        case "offline":
            return t("lib.net-offline", "无法连接到 {{site}}，检查网络或代理后重试", { site });
        case "timeout":
            return t("lib.net-timeout", "{{site}} 响应太慢，稍后再试", { site });
        case "busy":
            return t("lib.net-busy", "{{site}} 请求太频繁，过一会儿再试", { site });
        case "denied":
            return t("lib.net-denied", "{{site}} 拒绝了这次请求", { site });
        case "notfound":
            return t("lib.net-notfound", "{{site}} 上没有找到对应内容", { site });
        case "server":
            return t("lib.net-server", "{{site}} 暂时不可用，稍后再试", { site });
        case "parse":
            // 后端那句 `Api` 不带 URL，代码里恒没有主机段 ⇒ 这句不说「谁」
            return t("lib.net-parse", "返回的数据读不懂，稍后再试");
        case "io":
            return t("lib.net-io", "本机读写文件失败，检查磁盘空间后重试");
        case "retry":
            return t("lib.net-retry", "下载重试了几次都没成功，检查网络后重试");
        case "http":
            // 认不出的状态码：只说「没完成」，数字留在 detail 里给诊断信息用
            return t("lib.net-http", "{{site}} 没能完成这次请求，稍后重试", { site, status });
        default:
            return t("lib.net-generic", "网络请求失败，稍后重试");
    }
}

/** `app:种类` → 界面那句话。命令层兜底码（阻塞任务崩了、系统程序没打开那个路径），同样不带路径 */
export function appText(code: string): string | null {
    if (!code.startsWith("app:")) return null;
    const kind = code.slice(4);
    // 更新那一轮的码先单走一张表：它的下一动作各不相同（等一等 / 重开一次 / 只能去发布页），
    // 混在兜底那句「本机操作没能完成」里，用户就只会一遍遍点同一个必失败的按钮
    if (kind.startsWith("update-")) return updateText(kind);
    switch (kind) {
        case "panic":
            return t("lib.app-panic", "本机处理没能完成，重试一次");
        case "open":
            return t("lib.app-open", "打不开那个位置，可能它已经被移动或删除");
        case "reveal":
            return t("lib.app-reveal", "没能定位到那个文件，可能它已经不在原来的位置");
        default:
            return t("lib.app-generic", "本机操作没能完成，稍后重试");
    }
}

/**
 * 取件（下载 + 验签）那一段的降级句。措辞分工是刻意的：
 * 说「去发布页」的四条是**发布侧或本机形态**的问题，用户在这扇窗里做什么都没用；
 * 说「重试」的是这条链自己的抖动，同一颗按钮再点一次就是正解。
 */
function updateText(kind: string): string {
    switch (kind) {
        case "tag":
            return t("update.fail-tag", "这一版的编号对不上格式，没能开始下载");
        case "version":
            return t("update.fail-version", "这条发布的版本号读不懂，请在发布页手动下载");
        case "busy":
            return t("update.fail-busy", "已经有一次更新在跑了，先等它结束");
        case "downgrade":
            return t("update.fail-downgrade", "本机已经是这一版或更新，不需要下载");
        case "channel-mismatch":
            return t("update.fail-channel", "这条发布不在你订阅的渠道里，没能下载");
        case "incomplete":
            return t("update.fail-incomplete", "这条发布缺安装包或签名，请在发布页手动下载");
        case "host-denied":
            return t("update.fail-host", "下载地址不在可信的站点，没能下载");
        case "io":
            return t("update.fail-io", "本机没能把安装包写下来，磁盘可能满了");
        case "canceled":
            return t("update.fail-canceled", "这次下载已经取消");
        case "short":
            return t("update.fail-short", "下载没取满就断了，重试一次");
        case "name":
            return t("update.fail-name", "这条发布的产物文件名不合规矩，没能下载");
        case "key-missing":
            return t("update.fail-key-missing", "这个构建没内置校验公钥，请在发布页手动下载");
        case "key-invalid":
            return t("update.fail-key-invalid", "内置的校验公钥读不懂，这一版没法验签");
        case "sig-format":
            return t("update.fail-sig-format", "签名文件读不懂，没能校验");
        case "sig-mismatch":
            return t("update.fail-sig-mismatch", "签名与安装包对不上，没能校验");
        default:
            return t("update.fail-generic", "这次更新没能完成，稍后重试");
    }
}

/** invoke reject 回来的可能是 Error，也可能是 Rust 的字符串消息。全站提示一律过这里。 */
export function errOf(e: unknown): string {
    const raw = e instanceof Error ? e.message : String(e);
    return (
        netText(raw) ??
        appText(raw) ??
        tSource(
            /*i18n:
                有任务正在转换或排队中，清空缓存会删掉它在用的文件
                找不到应用配置目录，设置未能保存
                这条任务已经不在列表里（可能刚被撤回或重复删除）
                回收站里已经没有这条任务（可能刚被清空）
                任务列表里已经有这条任务，撤回没有执行
            */
            raw
        )
    );
}

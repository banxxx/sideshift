/**
 * 失败信息怎么「说给人听」——全站只这一处。
 *
 * 病根：`DownloadError` 的 Display 长这样 `网络请求失败：{url}（HTTP {status}）`，它一路
 * `e.to_string()` 过 IPC，前端再插进「版本加载失败 · {{error}}」那类模板 ⇒ 界面上出现
 * 「无法连接 https://api.modrinth.com/v2/project/cloth-config/version（HTTP 0）」。两句废话
 * 拼一起，而且这句是**动态串**，进不了 zh→键 那张表（表里只有整句字面量）⇒ 三档语言永远露中文。
 *
 * 分工：后端只出**种类代码**（`net:offline:api.modrinth.com`，见 `core/downloader/types.rs`），
 * 本文件把代码换成本地化的一句话；URL、路径、状态码一律不上界面。要留着排查的原句走
 * `TaskError.detail`（任务详情那张卡的「复制诊断信息」读它，不读代码）。
 *
 * `errOf` 对认不出的字符串退回 `tSource`——后端还有一批写死的中文整句（闸门、存档失败…），
 * 那些本来就在表里，照常翻。
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

/** invoke reject 回来的可能是 Error，也可能是 Rust 的字符串消息。全站提示一律过这里。 */
export function errOf(e: unknown): string {
    const raw = e instanceof Error ? e.message : String(e);
    return (
        netText(raw) ??
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

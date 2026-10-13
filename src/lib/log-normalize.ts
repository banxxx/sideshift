/**
 * 日志行归一化：把后端原句与外部程序（本机 Java 安装器）的英文输出整理成「对象 · 病因」。
 * 三层判据，任何一层认不出都照原样给出——归类失败绝不能把排查要用的信息弄丢。
 * 整条 URL 不进界面（换成站名），原句留在 `evidence` 一起进复制文本。
 * 判据住在前端、认的是字符串：后端或安装器改了措辞就会退回原样显示，这是显示层收口的代价。
 */
import { netText, siteName } from "@/lib/errors";
import { t } from "@/lib/i18n";

export interface NormalizedLog {
    /** 给界面看的正文 */
    message: string;
    /** 后端/安装器的原句，只在正文被改写时才有值 */
    evidence?: string;
}

/** URL 的取值范围停在空白与中英括号、逗号之前：日志里 URL 后面总是跟着中文说明 */
const URL_RE = /https?:\/\/[^\s，,、)）（]+/g;
const FILE_RE = /([\w.+-]+\.(?:jar|zip|mrpack|json|7z|tar|gz|pak))/i;

/**
 * 整行里没有这些线索就不碰：少了这道闸，「本机安装超时」会被判成连接超时。
 * `Exception` 前面不许带 `\b`——安装器抛的是 `java.net.ConnectException` 这种连写名，
 * 带边界就只认得独立单词，那条正好是判不出病因的那一类。
 */
const NETWORK_SHAPED = /(https?:\/\/|Exception\b|HTTP\s|timed out|Couldn't connect)/i;

/**
 * 状态码族必须挨着 HTTP/状态码 这类字面才算数：日志里的裸三位数太多
 * （`已收 512 字节`、版本号、行数），少了这道闸 sha1 不一致会被判成「服务端出错」。
 * 词形句（Forbidden / Not Found…）不用前缀，安装器的英文原文里就是裸的。
 */
const CODE = "(?:HTTP|status|状态码|code)[^\\d]{0,4}";

/** 病因判据，有序，第一条命中即用；`kind` 对齐 `errors.ts` 的 `net:` 种类 */
const CAUSES: [RegExp, string][] = [
    [/SocketTimeoutException|ConnectTimeoutException|Read timed out|请求超时|timed out/i, "timeout"],
    [
        /UnknownHostException|NoRouteToHostException|ConnectException|Connection refused|Failed to establish|Couldn't connect|error sending request|SocketException|SSLException|certificate|网络请求失败|请求失败（HTTP 0）/i,
        "offline",
    ],
    [new RegExp(`Forbidden|${CODE}(?:401|403)\\b`, "i"), "denied"],
    [new RegExp(`Not Found|${CODE}404\\b`, "i"), "notfound"],
    [new RegExp(`rate limit|Too Many Requests|${CODE}429\\b`, "i"), "busy"],
    [new RegExp(`Bad Gateway|Service Unavailable|${CODE}5\\d{2}\\b`, "i"), "server"],
];

function hostOf(url: string): string {
    const tail = url.split("://")[1] ?? url;
    return tail.split(/[/?#]/)[0];
}

/** 那句「病因」：有站名走现成的种类表（三档译文齐），没站名只剩这一族通用句 */
function causeOf(kind: string, host: string | null): string | null {
    if (host) return netText(`net:${kind}:${host}`);
    switch (kind) {
        case "timeout":
            return t("lib.log-net-timeout", "下载超时，稍后再试");
        case "offline":
            return t("lib.log-net-offline", "连不上下载源，检查网络或代理后重试");
        default:
            return null;
    }
}

export function normalizeLog(raw: string): NormalizedLog {
    // 「安装器报错：」是我们自己加的前缀，正文重写后就不需要它了
    const line = raw.replace(/^安装器报错：/, "").trim();
    if (!NETWORK_SHAPED.test(line)) return { message: raw };

    const first = URL_RE.exec(line);
    URL_RE.lastIndex = 0;
    const host = first ? hostOf(first[0]) : null;
    const kind = CAUSES.find(([re]) => re.test(line))?.[1];
    const cause = kind ? causeOf(kind, host) : null;

    if (cause) {
        const name = FILE_RE.exec(line)?.[1];
        return { message: [name, cause].filter(Boolean).join(" · "), evidence: raw };
    }

    // 认不出病因，但整条 URL 至少不该杵在界面上；真没 URL 可换就连前缀都不动它
    const tidied = line.replace(URL_RE, (u) => siteName(hostOf(u)));
    return tidied === line ? { message: raw } : { message: tidied, evidence: raw };
}

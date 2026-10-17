/**
 * 日志行归一化：把后端原句与外部程序（本机 Java 安装器）的英文输出整理成「对象 · 病因」。
 * 三层判据，任何一层认不出都照原样给出——归类失败绝不能把排查要用的信息弄丢。
 * 整条 URL 不进界面（换成站名），原句留在 `evidence` 一起进复制文本。
 * 判据住在前端、认的是字符串：后端或安装器改了措辞就会退回原样显示，这是显示层收口的代价。
 *
 * 一行日志的取词口径在 `logText`：后端发的「模板 + 参数」（`messageMsg`）优先，按项目 i18n 设计翻成
 * 当前档；没有这一项（旧任务、外部进程原文）才落回 `normalizeLog` 那套形状判据。
 */
import { errOf, netText, siteName } from "@/lib/errors";
import { backendText, t, tSource } from "@/lib/i18n";
import type { BackendMsg } from "@/lib/types/l10n";
import type { TaskError, TaskLogLine } from "@/lib/types/task";

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

/** 认后端那句「模板 + 参数」（Rust: `l10n::Msg`，序列化后就是 key/args/zh 三个字段） */
function isBackendMsg(v: unknown): v is BackendMsg {
    // 字段来自 IPC，静态类型管不到运行时 ⇒ 这里按「可能是残缺的」查，Partial 就是给这个判据让路的
    return typeof v === "object" && v !== null && typeof (v as Partial<BackendMsg>).key === "string";
}

/**
 * 参数值 → 能交给 i18next 的那份。两件事只有在这里做得了：
 *  - 值是**另一句话**（后端把带参数的明细嵌进外层句子当参数）⇒ 连它一起按模板渲染；
 *  - 值是**登记过的整句**（任务标题、分组词那类固定中文）⇒ 先翻再插；文件名/版本号/路径查不中表，原样贴。
 * 数字与布尔照原样给（`count` 那类槽要走 i18next 的复数规则）。
 *
 * 下面这块登记的是后端当**槽值**发来的那几个固定词（`取件 · {{label}} …` 的分组词）：
 * 目录名之类的目标值不在这里，它们本就该原样贴出来。
 */
/*i18n:
    模组
    加载器
*/
function slotValue(v: unknown): unknown {
    if (typeof v === "string") return tSource(v);
    if (isBackendMsg(v)) return templateText(v.zh, v);
    return v;
}

/** 拿模板查目录渲染当前档的那句话；查不到（旧数据、这句还没登记）掉回整句 */
function templateText(raw: string, m: BackendMsg): string {
    const args = m.args
        ? Object.fromEntries(Object.entries(m.args).map(([k, v]) => [k, slotValue(v)]))
        : null;
    return backendText(raw, m.key, args);
}

/** 一行日志的两段：整句必在，「模板 + 参数」可能缺（旧任务、外部进程原文） */
type LogSource = Pick<TaskLogLine, "message"> & Partial<Pick<TaskLogLine, "messageMsg">>;

/**
 * 后端单独发来的一句话（验证的六步摘要、阻塞项、环境提示）：带模板就按当前档翻，
 * 没有模板（`Msg::foreign` 那类外部原文）照原句。
 * 日志行**不走这里**，那类还要按形状收一次（去 URL、认病因），入口是 `logText`。
 */
export function msgText(raw: string, m?: BackendMsg | null): string {
    return m?.key ? templateText(raw, m) : raw;
}

/**
 * 整理一行渲染结果，但**判据认的是中文原句的形状**，不是译文：
 * 「本机安装超时（300 秒）」翻成英文会带出 `timed out`，那是我们自己的句子、不是网络病因，
 * 拿译文去撞形状判据会把整句换成「下载超时，稍后再试」。原句本来不需要整理，译文就照原样用。
 */
function tidy(rawZh: string, translated: string): NormalizedLog {
    if (normalizeLog(rawZh).message === rawZh) return { message: translated };
    return normalizeLog(translated);
}

/**
 * 界面取一行日志的正文：后端那句「模板 + 参数」优先（这才翻得动带文件名/数字的句子），
 * 外部原文混在里面时（带 URL 的诊断串）照旧按形状收一次。
 * 没有模板的行（磁盘上的旧任务、`Msg::foreign` 那类）与改造前逐字一致。
 */
export function logText(line?: LogSource | null): NormalizedLog {
    const raw = line?.message ?? "";
    if (!raw) return { message: "" };
    const m = line?.messageMsg;
    if (!m?.key) return normalizeLog(raw);
    return tidy(raw, templateText(raw, m));
}

/** 失败信息的两段：种类码 + 明细（可选模板） */
type ErrorSource = Pick<TaskError, "detail"> & Partial<Pick<TaskError, "code" | "detailMsg">>;

/** 后端自己写了「模板 + 参数」的那一句，翻成当前档；没有这一项（旧任务、外部原文）返回 null */
function detailText(err: ErrorSource): string | null {
    const m = err.detailMsg;
    return m?.key ? templateText(err.detail, m) : null;
}

/**
 * 错误卡正文与列表页那句失败摘要共用的一行：种类码（`net:` / `app:`）优先，
 * 其次后端自己写的「模板 + 参数」，都没有才落到形状判据 + 「中文 → 键」表。
 * `short` = 列表那一行只放得下一句：翻完再按形状收一次（认不出病因的照原样，由外层截断）。
 */
export function errorText(err: ErrorSource, short = false): string {
    if (err.code) return errOf(err.code);
    const m = detailText(err);
    if (m !== null) return short ? errOf(tidy(err.detail, m).message) : m;
    return errOf(short ? normalizeLog(err.detail).message : err.detail);
}

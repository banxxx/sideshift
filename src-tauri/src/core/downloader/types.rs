//! 取件契约：一条待获取文件长什么样、从哪来、取回后报什么，以及跨层共用的错误类型。

use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("网络请求失败：{url}（HTTP {status}）")]
    Http { url: String, status: u16 },
    /// 单次请求在自己的预算内没走完（`METADATA_TIMEOUT` 掐的）。必须与「根本没连上」分开：
    /// 前者说「响应太慢」，后者才说「连不上、检查网络或代理」——混成后者会让人去查自己家代理，
    /// 而镜像站点慢恰恰是查不到的那种慢
    #[error("请求超时：{url}")]
    Timeout { url: String },
    #[error("文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据解析失败：{0}")]
    Api(#[from] serde_json::Error),
    #[error("下载失败（已重试 {attempts} 次）：{file_name} — {cause}")]
    Failed {
        file_name: String,
        attempts: u32,
        cause: String,
    },
    #[error("未找到可用版本：{0}")]
    NotFound(String),
    /// 平台在门口就把请求拒了（缺 API Key、Key 无效）。这类话必须原样给用户看，
    /// 不能套进 `Http` 那句「网络请求失败：{url}（HTTP 403）」——那是给日志看的，
    /// 用户读不出「403」其实等于「你还没填 Key」
    #[error("{0}")]
    Refused(String),
}

/// 只取主机名：`https://api.modrinth.com/v2/x?y=1` → `api.modrinth.com`。
/// 界面要说「谁没响应」，路径与查询串对使用者是噪声，对排查者也不如状态码有用。
pub fn host_of(url: &str) -> &str {
    let tail = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = tail.split(['/', '?', '#']).next().unwrap_or(tail);
    if host.is_empty() {
        "unknown"
    } else {
        host
    }
}

/// 状态码 → 失败种类。`0` = 根本没收到响应（断网 / DNS / 超时 / 被掐），
/// 对用户来说这三种的表现就是同一句话，所以不再分；认不出的码原样带上，界面上不显示，
/// 但会留在「复制诊断信息」里。
fn net_kind(status: u16) -> &'static str {
    match status {
        0 => "offline",
        401 | 403 => "denied",
        404 => "notfound",
        429 => "busy",
        500..=599 => "server",
        _ => "http",
    }
}

/// 一条稳定代码（`net:种类:主机[:状态码]`）：前端 `errOf()` 据此挑本地化文案。
pub fn net_code(url: &str, status: u16) -> String {
    let host = host_of(url);
    let kind = net_kind(status);
    if kind == "http" {
        return format!("net:{kind}:{host}:{status}");
    }
    format!("net:{kind}:{host}")
}

/// 不经 `Downloader` 的那几条 reqwest 调用（`check_update` 直连 GitHub）走同一套代码，
/// 否则界面会露出 reqwest 自己那句英文 `error sending request for url (...)`。
/// 分类只有 `net_err` 一个出口：两条路径不会说出两种话
pub fn reqwest_code(e: &reqwest::Error, url: &str) -> String {
    net_err(url, e).ipc_msg()
}

/// reqwest 的错误 → `DownloadError`：超时单独成一档，其余仍按「有没有状态码」分
/// （答了但答得不对 = 带状态码的 `Http`，根本没答 = 状态码 0）。
/// 每个 send/text/bytes 失败点都走这里，别再各自 `unwrap_or(0)`——那样超时会被念成「连不上」
pub fn net_err(url: &str, e: &reqwest::Error) -> DownloadError {
    if e.is_timeout() {
        return DownloadError::Timeout { url: url.to_string() };
    }
    DownloadError::Http {
        url: url.to_string(),
        status: e.status().map(|s| s.as_u16()).unwrap_or(0),
    }
}

/// 这一类失败值得在**同一条源**上再敲一次：超时、根本没接上、以及源自己的 5xx。
/// 4xx 不重（Key 无效、路径不存在，重试只是白等）；解析失败也不重（镜像给的是错误页，
/// 再要一次还是那页）——那两种直接换下一条候选
pub fn worth_retry(e: &DownloadError) -> bool {
    match e {
        DownloadError::Timeout { .. } => true,
        DownloadError::Http { status, .. } => *status == 0 || (500..=599).contains(status),
        _ => false,
    }
}

impl DownloadError {
    /// 能归类的出代码，归不了类的回 `None`（那句本来就是写给人看的中文，套码反而丢信息）
    pub fn net_code(&self) -> Option<String> {
        Some(match self {
            DownloadError::Http { url, status } => net_code(url, *status),
            DownloadError::Timeout { url } => format!("net:timeout:{}", host_of(url)),
            // `NotFound` 的 payload 有两种形状：一条 URL（client.rs 的「候选链全部没拿到」）
            // 和一个模组名/中文短语（`cloth-config`、「CurseForge 构建 x/y 的下载链接」）。
            // 只有前者能说出「是谁没找到」；把短语当主机名喂给界面会吐出一句乱码 ⇒ 归不了类，原句照旧
            DownloadError::NotFound(s) if s.contains("://") => net_code(s, 404),
            DownloadError::NotFound(_) => return None,
            DownloadError::Api(_) => "net:parse".to_string(),
            DownloadError::Io(_) => "net:io".to_string(),
            // attempts=0 的 `Failed` 是包内/本地读失败，不是网络的事，保留原句
            DownloadError::Failed { attempts: 0, .. } => return None,
            DownloadError::Failed { .. } => "net:retry".to_string(),
            DownloadError::Refused(_) => return None,
        })
    }

    /// 过 IPC 的错误载荷：有代码出代码，没代码出原本那句中文
    pub fn ipc_msg(&self) -> String {
        self.net_code().unwrap_or_else(|| self.to_string())
    }
}

/// 文件来源：远程 URL、本地 zip 包内条目（裸 zip 整合包免网络直提）、或本地单文件
#[derive(Debug, Clone)]
pub enum Fetch {
    Url(String),
    ZipEntry { archive: PathBuf, entry: String },
    Local(PathBuf),
}

/// 一个待获取文件
#[derive(Debug, Clone)]
pub struct ItemSpec {
    pub fetch: Fetch,
    pub file_name: String,
    pub sha1: Option<String>,
    /// 最终落盘绝对路径（含文件名）
    pub dest: PathBuf,
    /// 源文件大小（字节）；0 = 未知。构建不消费此字段，仅供下载量预估聚合
    pub size_bytes: u64,
}

impl ItemSpec {
    pub(crate) fn source_key(&self) -> String {
        match &self.fetch {
            Fetch::Url(u) => u.clone(),
            Fetch::ZipEntry { archive, entry } => format!("{}#{entry}", archive.display()),
            Fetch::Local(p) => p.to_string_lossy().to_string(),
        }
    }
}

/// 取件来源（界面据此区分「下载」与「取件」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchSource {
    /// 需联网
    Network,
    /// 整合包内直取
    Pack,
    /// 本地 jar 复制
    Local,
}

/// 单条目取件结果：联网项逐条报，离线项由流水线按落位目录聚合后再报
#[derive(Debug, Clone)]
pub struct ItemOutcome {
    pub file_name: String,
    pub source: FetchSource,
    pub cached: bool,
    pub bytes: u64,
    /// 联网重试次数（0 = 一次成功）
    pub retries: u32,
    /// 实际落位绝对路径：流水线据此归入「模组 / 各保留目录」分组
    pub dest: PathBuf,
}

/// 逐条完成回调：Arc 持有与借用两种传法共用同一签名
pub(crate) type OnDone = dyn Fn(usize, usize, &ItemOutcome) + Send + Sync;

/// 联网传输中的字节进度（每个响应块一次，调用方自行节流；离线搬运不发）
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub file_name: String,
    /// 本条目的唯一键（dest 绝对路径）：并发下载同名条目也不串账
    pub key: PathBuf,
    /// 本文件已收字节
    pub done: u64,
    /// 本文件总字节，0 = 响应无 Content-Length
    pub total: u64,
    /// 第几次尝试（1 起）
    pub attempt: u32,
}

pub(crate) type OnTransfer = dyn Fn(&TransferProgress) + Send + Sync;

#[cfg(test)]
mod tests {
    use super::*;

    fn http(url: &str, status: u16) -> DownloadError {
        DownloadError::Http {
            url: url.to_string(),
            status,
        }
    }

    #[test]
    fn only_the_host_survives_into_the_code() {
        // 这条断言就是本次改动的靶心：路径与查询串不许跟着进界面
        assert_eq!(
            net_code("https://api.modrinth.com/v2/project/cloth-config/version?page=1", 0),
            "net:offline:api.modrinth.com"
        );
        assert_eq!(host_of("api.curseforge.com/v1/b"), "api.curseforge.com");
        assert_eq!(host_of(""), "unknown");
    }

    #[test]
    fn status_codes_fold_into_kinds_and_the_rest_keep_the_number() {
        assert_eq!(net_code("u", 403), "net:denied:u");
        assert_eq!(net_code("u", 401), "net:denied:u");
        assert_eq!(net_code("u", 404), "net:notfound:u");
        assert_eq!(net_code("u", 429), "net:busy:u");
        assert_eq!(net_code("u", 503), "net:server:u");
        // 认不出的码：种类是 http、原值留在最后一段（界面不显示，诊断信息里有）
        assert_eq!(net_code("u", 418), "net:http:u:418");
    }

    #[test]
    fn errors_that_are_already_plain_chinese_stay_verbatim() {
        // 缺 Key 那句要给「去设置里填」的出口，套上代码就把意思抽掉了
        let refused = DownloadError::Refused("还没有配置 CurseForge API Key".into());
        assert_eq!(refused.net_code(), None);
        assert_eq!(refused.ipc_msg(), "还没有配置 CurseForge API Key");
        // 没重试过的 Failed = 包内/本地读失败，不是网络的事
        let local = DownloadError::Failed {
            file_name: "x.jar".into(),
            attempts: 0,
            cause: "包内没有这个条目".into(),
        };
        assert_eq!(local.net_code(), None);
        // NotFound 装的是模组名/中文短语时，那句已经是给人看的话，别把「构建 x/y」当域名说出去
        let named = DownloadError::NotFound("CurseForge 构建 1/2 的下载链接".into());
        assert_eq!(named.net_code(), None);
        assert_eq!(named.ipc_msg(), "未找到可用版本：CurseForge 构建 1/2 的下载链接");
    }

    #[test]
    fn network_shaped_errors_become_codes() {
        assert_eq!(
            http("https://meta.modrinth.cn/x", 500).ipc_msg(),
            "net:server:meta.modrinth.cn"
        );
        assert_eq!(
            DownloadError::NotFound("https://api.modrinth.com/x".into()).ipc_msg(),
            "net:notfound:api.modrinth.com"
        );
        assert_eq!(
            DownloadError::Api(serde_json::from_str::<u8>("not json").unwrap_err()).ipc_msg(),
            "net:parse"
        );
        let retried = DownloadError::Failed {
            file_name: "x.jar".into(),
            attempts: 3,
            cause: "https://y 请求失败（HTTP 500）".into(),
        };
        assert_eq!(retried.ipc_msg(), "net:retry");
    }
}

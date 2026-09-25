//! 取件契约：一条待获取文件长什么样、从哪来、取回后报什么，以及跨层共用的错误类型。

use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("网络请求失败：{url}（HTTP {status}）")]
    Http { url: String, status: u16 },
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

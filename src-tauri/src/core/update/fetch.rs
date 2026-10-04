//! 取件：按 tag 重新解析 release → 流式下载 → minisign 验签 → 改名定稿。
//!
//! §8 的三条安全硬规在这一块都有落点，而且各只在一处：
//! 1. **只下这一条 release 里配齐的那一对**，址从**这一次**响应里现取（不复用列表那份，见 `release_tag_url`）；
//! 2. **宿主必须在白名单**（`host_allowed`，由 `release::parse` 在解析时打上 `trusted`）；
//! 3. **验的是落地字节的哈希**（`verify::verify_file`），所以对端哪怕伪造整份响应也没用。
//!
//! 状态放在进程级单例里，不放 `task_engine::Inner` 的全局锁：更新这一轮与转换任务无关，
//! 它该在任务引擎忙的时候也能被取消；把它塞进那把锁就等于让「清缓存扫了 8 秒」期间点不动取消。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures::StreamExt;
use reqwest::Client;
use tauri::{AppHandle, Emitter};

use super::release::{self, Asset};
use super::verify::{self, KEY_MISSING};
use super::{host_allowed, release_tag_url, updater_pubkey};
use crate::core::downloader::{
    app_code, net_code, net_timeout_code, reqwest_code, PART_MARKER, USER_AGENT,
};
use crate::models::{UpdateChannel, UpdateStage, UpdateStatus};

/// 缓存里的第 6 类：`{cache}\update\{版本}\`。**目录名只在这里写一次**，清理侧引的是这个常量
pub const CACHE_UPDATE_DIR: &str = "update";

/// 进度与阶段的事件名。一条载荷 = 整个 `UpdateStatus`，前端不必把两张表拼起来才知道「在下第几版」
pub const EVENT_PROGRESS: &str = "update://progress";

/// 两个响应块之间最多干等多久：过了就算停滞，换一次重试。
/// 这里刻意**不给整条请求设超时**——reqwest 的 `.timeout()` 盖的是「含读完响应体」的全过程，
/// 而十几 MB 的包在弱网下跑几分钟本来就是对的，设了等于把正常下载判死（模组那条链的 120s 是另一套预算）
const STALL: Duration = Duration::from_secs(30);

/// 同一个产物重试几次。GitHub 的资产址不带短时效签名，同一发请求重试是有意义的
const RETRIES: u32 = 3;

/// 重试前的间隔（线性）：与模组那条链同一个口径，长过这个就开始像「卡住了」
const BACKOFF: Duration = Duration::from_millis(500);

/// 进度事件的最小间隔。每个块发一条会把前端刷成打字机，100ms 一档足够画出进度条的形状
const TICK: Duration = Duration::from_millis(100);

/// 一次尝试的失败分类（与 `downloader::client` 同一套思路，只是这里没有「换源」那一档：P5 才有多源）
enum Stop {
    /// 换个时间再试可能成功（连接断、408/429/5xx、块间停滞、收回来是半截）
    Retry(String),
    /// 重试也不会变好（404/403、磁盘写不进、用户取消）：立即结束这一轮
    Fatal(String),
}

/// 进程级那一轮。同时最多一轮：两个并发下载往同一个版本目录写，出事时说不清是谁干的
struct Session {
    status: UpdateStatus,
    running: bool,
    cancel: Arc<AtomicBool>,
    /// 定稿那一刻盘上的安装包路径。**只有 `Ready` 档才不是 None**（`settle` 每次换档都清它）。
    /// 装那一跳读的就是这一格，而不是去扫目录猜文件名：扫出来的那套判据会和下载侧分叉，
    /// 分叉的结果是「下载得下来、装不了」，而那句在界面上一句都修不了
    package: Option<PathBuf>,
}

impl Session {
    fn new() -> Self {
        Self {
            status: UpdateStatus::idle(),
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            package: None,
        }
    }
}

fn session() -> &'static Mutex<Session> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Session::new()))
}

/// 现在这一档（`update_status` 命令用：弹窗重开、或界面冷启动时要能问出「上一轮办到哪」）
pub fn status() -> UpdateStatus {
    session().lock().unwrap().status.clone()
}

/// 有没有正在进行的取件。清理侧据此不碰那一轮正在写的 `.part`（Windows 上攥着句柄的文件删不掉，
/// 报了 failed 只会让用户以为清不干净）
pub fn busy() -> bool {
    session().lock().unwrap().running
}

/// 收掉某一行已经定稿的文件，回 `idle`。
/// 下载进行中的取消走的是另一条路（立旗 + 由那一轮自己删自己的半截），所以这里不动 running。
pub fn cancel(cache_dir: &Path) -> UpdateStatus {
    let mut s = session().lock().unwrap();
    if s.running {
        s.cancel.store(true, Ordering::Relaxed);
        s.status.stage = UpdateStage::Canceled;
        s.status.error = None;
        s.package = None;
        return s.status.clone();
    }
    // 没在跑：这一句「取消」说的是把这一版从盘上收掉，状态回 idle
    let version = s.status.version.clone();
    s.status = UpdateStatus::idle();
    s.package = None;
    drop(s);
    if let Some(v) = version {
        discard(cache_dir, &v);
    }
    UpdateStatus::idle()
}

/// 删掉某个版本的暂存目录。删不动不报错：那是缓存，不是账本，留着下次清理照样收得走。
/// 装成功后的下一次启动也用它（`install::take_outcome` 那一跳）：那一对字节这时候已经变成
/// 装进机器里的程序，留在缓存里只是白占 4–20 MB
pub fn discard(cache_dir: &Path, version: &str) {
    let dir = cache_dir.join(CACHE_UPDATE_DIR).join(version);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 写状态并广播。阶段与错误都从这里走，界面看到的与实际发生的是同一条。
/// 换档即撤掉 `package`：那一格说的是「盘上这一份已经验过」，一旦不再是 Ready 就不能再有人拿它去装
fn settle(app: &AppHandle, stage: UpdateStage, error: Option<String>) -> UpdateStatus {
    let st = {
        let mut s = session().lock().unwrap();
        s.status.stage = stage;
        s.status.error = error;
        s.package = None;
        s.status.clone()
    };
    let _ = app.emit(EVENT_PROGRESS, &st);
    st
}

/// 定稿：钉在 `Ready`，并把「装的是盘上哪一个文件」一起记进这一轮。
/// 与 `settle` 分开写，是因为 `Ready` 是唯一带产物的一档——两处共用就会在每个阶段都得想一遍
/// 「这一档到底该不该有路径」
fn landed(app: &AppHandle, pkg_path: &Path) -> UpdateStatus {
    let st = {
        let mut s = session().lock().unwrap();
        s.package = Some(pkg_path.to_path_buf());
        s.status.stage = UpdateStage::Ready;
        s.status.error = None;
        s.status.clone()
    };
    let _ = app.emit(EVENT_PROGRESS, &st);
    st
}

/// 已验签、等着被装的那一对（版本号, 安装包路径）。`None` = 现在没有可装的东西
/// （没定稿、或这一轮已经被取消/失败撤掉）。装那一跳的唯一入口读它
pub fn ready_package() -> Option<(String, PathBuf)> {
    let s = session().lock().unwrap();
    if s.status.stage != UpdateStage::Ready {
        return None;
    }
    let version = s.status.version.clone()?;
    let pkg = s.package.clone()?;
    Some((version, pkg))
}

/// 失败出口：状态钉在 `failed`、事件发出去，错误码原样回给调用方（命令层把它交给 `guard`）
fn fail(app: &AppHandle, code: String) -> String {
    settle(app, UpdateStage::Failed, Some(code.clone()));
    code
}

/// 报一次已收字节（累计口径：签名那几百字节也算在里面，进度条不会因为换了文件而回跳）
fn progress(app: &AppHandle, downloaded: u64) {
    let st = {
        let mut s = session().lock().unwrap();
        s.status.downloaded = downloaded;
        s.status.clone()
    };
    let _ = app.emit(EVENT_PROGRESS, &st);
}

/// 这一轮办的是哪个版本、总共多少字节。定稿前这两格会一直改
fn aim(version: &str, total: u64) {
    let mut s = session().lock().unwrap();
    s.status.version = Some(version.to_string());
    s.status.total = total;
}

/// tag 能不能原样拼进 URL 路径。
///
/// 它是前端递回来的外部输入，而这里是**拼路径**不是转义：放行范围收窄到 GitHub 实际用得着的那几个
/// 字符（字母数字与 `._-`），于是 `..`、`?`、`#`、`%`、空白、换行一概进不来。
/// 合法 tag（`v1.0.0`、`1.0.0-beta.2`）全在这套字符里，所以收窄不损失任何可达性。
/// 带 build 元数据的 `v1.0.0+build.9` 也在拒收之列：发布链从不出这种 tag，见到 `+` 只说明
/// 那一串不是我们发出去的东西。
pub fn tag_ok(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 64
        && !tag.starts_with('.')
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 把 release 给的文件名安全地摆进暂存目录。
///
/// `asset.name` 是远端字段，直接 `dir.join(name)` 就等于让一条 release 决定我们往哪儿写文件
/// （`..\\..\\..\\Startup\\xxx` 那种）。两条判据：
/// 1. **它必须自己就是一个完整文件名**——`file_name()` 回原串 ⇒ 里面没有分隔符、不是 `.`/`..`、也不空；
/// 2. **不含控制字符**——换行/制表在 Windows 上不是路径分隔符，第 1 条拦不住它，
///    而 `\\n` 这种名字落盘后既读不回也删不掉（`File::create` 那侧才报错，报的是 io，说不清是磁盘的事）
fn staged_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let p = Path::new(name);
    if p.file_name().and_then(|s| s.to_str()) != Some(name) || name.chars().any(|c| c.is_control()) {
        return Err(app_code("update-name"));
    }
    Ok(dir.join(name))
}

fn client() -> &'static Client {
    static C: OnceLock<Client> = OnceLock::new();
    C.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(STALL)
            .build()
            .expect("reqwest client")
    })
}

/// 取一件事前那四道闸（都不花流量）：没内置公钥、版本号说不清、跨渠道、降级
/// —— 都在这一步拦下，返回 Err 而不是"已失败的一轮"。
pub async fn prepare(
    app: &AppHandle,
    cache_dir: &Path,
    channel: UpdateChannel,
    current: &str,
    tag: &str,
) -> Result<UpdateStatus, String> {
    if !tag_ok(tag) {
        return Err(app_code("update-tag"));
    }
    if updater_pubkey().is_none() {
        return Err(app_code(KEY_MISSING));
    }
    let cur = semver::Version::parse(current.trim_start_matches('v'))
        .map_err(|_| app_code("update-version"))?;

    // 占位：running 位没人抢才算这一轮开起来。之后任何一条出口都要把 running 落下（下面的 tail 闭包）
    let cancel = {
        let mut s = session().lock().unwrap();
        if s.running {
            return Err(app_code("update-busy"));
        }
        s.running = true;
        s.cancel = Arc::new(AtomicBool::new(false));
        s.status = UpdateStatus {
            stage: UpdateStage::Downloading,
            version: Some(tag.to_string()),
            downloaded: 0,
            total: 0,
            error: None,
        };
        s.package = None;
        s.cancel.clone()
    };

    let out = run(app, cache_dir, channel, &cur, tag, &cancel).await;
    session().lock().unwrap().running = false;
    out
}

async fn run(
    app: &AppHandle,
    cache_dir: &Path,
    channel: UpdateChannel,
    cur: &semver::Version,
    tag: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<UpdateStatus, String> {
    let url = release_tag_url(tag);
    let body = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| fail(app, reqwest_code(&e, &url)))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| fail(app, reqwest_code(&e, &url)))?;
    let rel = release::parse_one(&body, &url).map_err(|c| fail(app, c))?;

    // 渠道与新旧各一判：前端递上来的 tag 只是「它想要这一版」，凭什么给由这里说了算
    if !release::belongs(&rel, channel) {
        return Err(fail(app, app_code("update-channel-mismatch")));
    }
    if rel.version <= *cur {
        return Err(fail(app, app_code("update-downgrade")));
    }
    // 缺件/不可信宿主在这里再看一次：`check_update` 那份结论可能已经过期（有人在中间编辑了 release）
    if rel.gap().is_some() {
        return Err(fail(app, app_code("update-incomplete")));
    }
    let (pkg, sig) = rel.artifacts().ok_or_else(|| fail(app, app_code("update-incomplete")))?;
    // 取址前最后一道：响应里回传的址必须仍在白名单。`trusted` 已经是这条判据的结果，
    // 这里再点一次名，是为了让「下载用的址」这条线上看得见这道闸（不是靠注释信任上游）
    if !host_allowed(&pkg.url) || !host_allowed(&sig.url) {
        return Err(fail(app, app_code("update-host-denied")));
    }

    let version = rel.version.to_string();
    let dir = cache_dir.join(CACHE_UPDATE_DIR).join(&version);
    let pkg_path = staged_path(&dir, &pkg.name).map_err(|c| fail(app, c))?;
    let sig_path = staged_path(&dir, &sig.name).map_err(|c| fail(app, c))?;
    aim(&version, pkg.size + sig.size);

    // 盘上已经有这一对 ⇒ 先验它。这就是「断点」的正解：不靠账本、不猜进度，
    // 落地字节的哈希过了才算数（一次读盘几十毫秒，比重新下十几 MB 便宜两个量级）
    if pkg_path.exists() && sig_path.exists() {
        settle(app, UpdateStage::Verifying, None);
        match landed_ok(&pkg_path, &sig_path) {
            Ok(()) => return Ok(landed(app, &pkg_path)),
            Err(_) => {
                // 对不上就是被人动过、或上次是半截：这一对已经废了，删掉重取。
                // 留着它，用户每点一次都是同一句「验签没过」，而那一句在界面上谁也修不了
                let _ = std::fs::remove_file(&pkg_path);
                let _ = std::fs::remove_file(&sig_path);
            }
        }
    }

    std::fs::create_dir_all(&dir).map_err(|_| fail(app, app_code("update-io")))?;
    settle(app, UpdateStage::Downloading, None);

    // 先下小的那一枚（几百字节）：签名到手才有「验落地字节」的前提，
    // 万一它拿不到，不必先白跑掉十几 MB
    let got = fetch_asset(app, cancel, sig, &sig_path, 0)
        .await
        .map_err(|c| fail(app, c))?;
    let got = fetch_asset(app, cancel, pkg, &pkg_path, got)
        .await
        .map_err(|c| fail(app, c))?;
    progress(app, got);

    settle(app, UpdateStage::Verifying, None);
    landed_ok(&pkg_path, &sig_path).map_err(|c| {
        // 验不过的包留在盘上只会让下一次点同一条死路：收掉，下一轮重新取
        let _ = std::fs::remove_file(&pkg_path);
        let _ = std::fs::remove_file(&sig_path);
        fail(app, c)
    })?;
    Ok(landed(app, &pkg_path))
}

/// 落地的那一对自不自洽（签名解得开 + 包的哈希验得过）。错误码由 `verify` 那两侧给。
/// `install` 在 spawn 前用同一句再验一次：从「已就位」到「点了安装」之间隔的是用户的手，
/// 那段时间里盘上那两个字节属于本机任何进程
pub(crate) fn landed_ok(pkg_path: &Path, sig_path: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(sig_path).map_err(|_| app_code("update-io"))?;
    let sig = verify::decode_signature(&raw)?;
    verify::verify_file(pkg_path, &sig)
}

/// 取一个资产：重试几次、每次都是流式收进 `.part` 再改名定稿。返回收到的字节数
async fn fetch_asset(
    app: &AppHandle,
    cancel: &Arc<AtomicBool>,
    asset: &Asset,
    dest: &Path,
    base: u64,
) -> Result<u64, String> {
    let mut last: Option<String> = None;
    for attempt in 1..=RETRIES {
        match stream_into(app, cancel, asset, dest, base, attempt).await {
            Ok(bytes) => return Ok(bytes),
            Err(Stop::Fatal(code)) => return Err(code),
            Err(Stop::Retry(cause)) => {
                last = Some(cause);
                if attempt < RETRIES {
                    if canceled(cancel) {
                        return Err(app_code("update-canceled"));
                    }
                    tokio::time::sleep(BACKOFF * attempt).await;
                }
            }
        }
    }
    // 三次都没拿下：报最后一次的原因。理论上到不了这条（循环至少跑一次），
    // 到了就说明那边一个字节都没回过——那正是「连不上」那一档
    Err(last.unwrap_or_else(|| net_code(&asset.url, 0)))
}

/// 一次尝试：连上、收流写 `.part`、按声明大小核对、改名。失败一律清掉自己那半个文件
async fn stream_into(
    app: &AppHandle,
    cancel: &Arc<AtomicBool>,
    asset: &Asset,
    dest: &Path,
    base: u64,
    attempt: u32,
) -> Result<u64, Stop> {
    let temp = dest.with_file_name(format!(
        "{}{PART_MARKER}{attempt}",
        dest.file_name().and_then(|s| s.to_str()).unwrap_or("download")
    ));
    let resp = client()
        .get(&asset.url)
        .send()
        .await
        .map_err(|e| Stop::Retry(reqwest_code(&e, &asset.url)))?;
    let status = resp.status();
    if !status.is_success() {
        // 走到这里时 3xx 已经被 reqwest 跟完了（默认最多 10 跳），所以「非 2xx」就是终态
        let code = net_code(&asset.url, status.as_u16());
        let code = if status.is_server_error() || matches!(status.as_u16(), 408 | 429) {
            Stop::Retry(code)
        } else {
            // 404/403 说的是「东西不在那儿」，重试只是白敲门
            Stop::Fatal(code)
        };
        return Err(code);
    }

    let mut file = std::fs::File::create(&temp).map_err(|_| Stop::Fatal(app_code("update-io")))?;
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    let mut last_tick = Instant::now();
    loop {
        if canceled(cancel) {
            drop(file);
            let _ = std::fs::remove_file(&temp);
            return Err(Stop::Fatal(app_code("update-canceled")));
        }
        let chunk = match tokio::time::timeout(STALL, stream.next()).await {
            Err(_) => {
                // 那边连着但不回字节了。半截文件留不住（下一次尝试换个临时名，旧的成垃圾），删
                drop(file);
                let _ = std::fs::remove_file(&temp);
                return Err(Stop::Retry(net_timeout_code(&asset.url)));
            }
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(Stop::Retry(reqwest_code(&e, &asset.url))),
            Ok(Some(Ok(b))) => b,
        };
        // 写完这块再判取消：块与块之间删文件，Windows 上句柄已释放，删得干净
        file.write_all(&chunk)
            .map_err(|_| Stop::Fatal(app_code("update-io")))?;
        written += chunk.len() as u64;
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            progress(app, base + written);
        }
    }
    file.flush().map_err(|_| Stop::Fatal(app_code("update-io")))?;
    drop(file);

    // 大小对不上 = 收的是半截（对面没报错、流自己就结束了）。这不算成功：
    // 验签当然也拦得住，但「少了几 MB」与「被人改过」是两句话，不该合成一句
    if asset.size > 0 && written != asset.size {
        let _ = std::fs::remove_file(&temp);
        return Err(Stop::Retry(app_code("update-short")));
    }
    std::fs::rename(&temp, dest).map_err(|_| Stop::Fatal(app_code("update-io")))?;
    Ok(written)
}

fn canceled(cancel: &Arc<AtomicBool>) -> bool {
    cancel.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tag 是拼进路径的外部输入：放行范围收窄到合法 tag 用得着的那几个字符，
    /// 于是穿越、查询、片段、换行都进不来
    #[test]
    fn only_real_tags_reach_the_url() {
        for ok in ["v1.0.0", "1.0.0", "v1.0.0-beta.2", "v1.0.0-rc.1"] {
            assert!(tag_ok(ok), "{ok} 是合法 tag");
        }
        for bad in [
            "",
            "..",
            "../..",
            "v1.0.0?foo=1",
            "v1.0.0#frag",
            "a%20b",
            "v1\n",
            "..\\x",
            "v1.0.0 /x",
            // 发布链从不出 build 元数据：见到 `+` 只说明那串不是我们发出去的
            "v1.0.0+build.9",
        ] {
            assert!(!tag_ok(bad), "{bad:?} 不该进 URL");
        }
    }

    /// release 给的文件名不许决定我们往哪儿写
    #[test]
    fn asset_names_carry_their_own_directory() {
        let dir = Path::new("cache/update/1.0.0");
        assert_eq!(
            staged_path(dir, "SideShift_1.0.0_x64-setup.exe").unwrap(),
            dir.join("SideShift_1.0.0_x64-setup.exe")
        );
        for bad in ["", ".", "..", "a/b.zip", "..\\..\\x.zip", "c:\\x.zip", "x\ny.zip"] {
            assert_eq!(staged_path(dir, bad).err(), Some(app_code("update-name")));
        }
    }

    /// 暂存目录按版本整只收走；目录本来就不存在也不算失败（那是缓存，不是账本）
    #[test]
    fn discard_takes_the_whole_version_dir() {
        let dir = std::env::temp_dir().join(format!("sideshift-fetch-discard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = dir.join(CACHE_UPDATE_DIR).join("1.2.3");
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(v.join("SideShift_1.2.3_x64-setup.exe"), b"zip").unwrap();
        std::fs::write(v.join("SideShift_1.2.3_x64-setup.exe.part1"), b"half").unwrap();

        discard(&dir, "1.2.3");
        assert!(!v.exists(), "半截与定稿一起收走");
        assert!(dir.join(CACHE_UPDATE_DIR).exists(), "上一级留着：别的版本可能还在");

        discard(&dir, "9.9.9"); // 没有这一版也不许报错
        std::fs::remove_dir_all(&dir).ok();
    }
}

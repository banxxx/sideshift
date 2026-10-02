//! minisign 验签：P1 的第一块地基，也是整套更新里唯一把「可信根」落到代码里的地方。
//!
//! 验的是**真正写进磁盘的那份字节的哈希**，不是 release 里的任何字段（安全硬规 4）。
//! 这条决定撑起了后面两期：P5 的镜像候选链不需要额外信任——镜像哪怕伪造整份响应也没用，
//! 签名对不上就止步在这里；§4 的降级态也不需要往 release 正文里塞 JSON 摘要，多一条契约
//! 只会多一个「发版时忘了填」的口子。
//!
//! 读盘走 1 MiB 分块的流式哈希：安装包是十几 MB 级，整份进内存换一个「能验」不值当，
//! 而且这条链的下游还要跑安装器，常驻不该由我们垫。

use std::io::Read;
use std::path::Path;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};

use super::updater_pubkey;
use crate::core::downloader::app_code;

/// 流式哈希的块大小（1 MiB）：十几 MB 的产物十几次读盘，缓冲区与块数都不刺眼
const CHUNK: usize = 1024 * 1024;

/// 「这个构建里没内置公钥」的种类码。取址那一跳（`fetch`）在花钱下载**之前**也要报同一句：
/// 验不了的包不该先跑掉用户十几 MB 的流量。字符串只在这颗常量里出现一次
pub const KEY_MISSING: &str = "update-key-missing";

/// 签名文本 → `Signature`。
///
/// 官方 `tauri signer sign` 落盘、并由 tauri-action 原样上传成 release 资产的 `.sig`，
/// **不是** minisign 那四行明文，而是「整份四行块再 base64」的一行形态
/// （本机实测：`tests/fixtures/update/fixture.txt.sig`，396 字节、零换行）。
/// 四行明文形态也收：这里放宽的只有**编码**，一个字节都没放宽的是校验——两种都还要过同一道验签。
pub fn decode_signature(raw: &str) -> Result<Signature, String> {
    let trimmed = raw.trim();
    let text = if trimmed.lines().count() > 1 {
        // 已经是四行明文（手撕过、或别的工具产出的）
        trimmed.to_string()
    } else {
        let bytes = b64(trimmed).ok_or_else(|| app_code("update-sig-format"))?;
        String::from_utf8(bytes).map_err(|_| app_code("update-sig-format"))?
    };
    // minisign 那句英文（"Invalid encoding in minisign data" 之类）不许抬到界面：按种类码归
    Signature::decode(&text).map_err(|_| app_code("update-sig-format"))
}

/// 公钥文本 → `PublicKey`，与 `decode_signature` 同一类**只放宽编码、不放宽校验**。
///
/// 这里有两种可达形态，而且**内置那一枚是我们自己写的，两种都该吃**，否则改配置格式就得改代码：
/// - 一行 base64 的**两行公开钥块**：`tauri signer generate` 落的 `.pub`，也是
///   `plugins.updater.pubkey` 该填的那一份（本机实测：夹具 152 字节、零换行，解出来是
///   `untrusted comment: minisign public key: …` + 42 字节裸钥的 base64）。
/// - 一行 base64 的**裸钥**（42 字节）：minisign 自己的 `minisign -K` 给的就是这一种短形。
///
/// `minisign_verify::PublicKey::from_base64` 只认第二种，把第一种喂给它必然 `InvalidEncoding`
/// ——这条就是本轮踩过的坑，别从函数名推它吃什么。
pub fn decode_public_key(raw: &str) -> Result<PublicKey, String> {
    let trimmed = raw.trim();
    let err = || app_code("update-key-invalid");
    // 短形先试：一行且 base64 解出恰好 42 字节 ⇒ 裸钥
    if let Some(n) = stripped_if_single_line(trimmed) {
        if b64_len(n) == Some(42) {
            return PublicKey::from_base64(n).map_err(|_| err());
        }
    }
    // 否则是「块」：可能本来就是两行明文，也可能是整块再 base64（官方那一种）
    let text = match b64(trimmed) {
        Some(bytes) => String::from_utf8(bytes).map_err(|_| err())?,
        None => trimmed.to_string(),
    };
    PublicKey::decode(&text).map_err(|_| err())
}

/// 一行且非空时返回它自己（裸钥短形的判据），多行返回 `None`
fn stripped_if_single_line(s: &str) -> Option<&str> {
    (s.lines().count() == 1 && !s.is_empty()).then_some(s)
}

/// base64 → 字节；解不开回 `None`（调用方各自归类，两种错误码不一样）
fn b64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

fn b64_len(s: &str) -> Option<usize> {
    b64(s).map(|v| v.len())
}

/// 用这一枚包内置的公钥验一个落地文件。没内置公钥时是 `update-key-missing`，不是「验不过」。
/// 两者在界面上是两句话：前者说的是我们这边还没接上密钥，后者说的是产物被人动过。
pub fn verify_file(path: &Path, sig: &Signature) -> Result<(), String> {
    match updater_pubkey() {
        Some(key) => verify_file_with(path, sig, key),
        None => Err(app_code(KEY_MISSING)),
    }
}

/// 验签的实际实现（公钥显式传入：测试要能用自己的那把钥，而不是改全局）
pub fn verify_file_with(path: &Path, sig: &Signature, pubkey: &str) -> Result<(), String> {
    let key = decode_public_key(pubkey)?;
    // 流式档只吃 prehashed：官方 signer 产的正是 prehashed（本机实测，见 tests 那条），
    // 老版 minisign 的 legacy 档直接拒——它验的不是「这份字节的哈希」而是字节本身，
    // 我们不给它留后门
    let mut verifier = key
        .verify_stream(sig)
        .map_err(|_| app_code("update-sig-format"))?;
    let mut file = std::fs::File::open(path).map_err(|_| app_code("update-io"))?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buf).map_err(|_| app_code("update-io"))?;
        if read == 0 {
            break;
        }
        verifier.update(&buf[..read]);
    }
    verifier.finalize().map_err(|_| app_code("update-sig-mismatch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用的一次性钥：公钥与签名都是公开产物，私钥只活在生成它的那台机器的 `.scratch/` 里，
    /// **没有**进仓库，也不该被当成发版钥。
    const PUBKEY: &str = include_str!("../../../tests/fixtures/update/test-key.pub");
    const SIG: &str = include_str!("../../../tests/fixtures/update/fixture.txt.sig");
    const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/update/fixture.txt");

    fn fixture_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/update/fixture.txt")
    }

    /// 官方 CLI 那一行 base64 的 `.sig` 解得开，而且解出来是 prehashed 档
    /// （§12 第 4 条实测的落点：`minisign-verify` 0.3 的第三个参数是 `allow_legacy`，
    ///  预发布位由签名自己的算法头 `ED` 决定，不是调用方选的）
    #[test]
    fn official_sig_is_prehashed_and_decodes() {
        let sig = decode_signature(SIG).expect("官方 .sig 该解得开");
        // legacy 档会在 verify_stream 那一步就回错；能建成 verifier 就说明是 prehashed
        let key = decode_public_key(PUBKEY).unwrap();
        assert!(key.verify_stream(&sig).is_ok());
    }

    /// 落地字节与签名对上 ⇒ 过；改一个字节 ⇒ 不过。这一条是整个更新链的信任根
    #[test]
    fn verify_passes_for_the_signed_bytes_only() {
        let sig = decode_signature(SIG).unwrap();
        verify_file_with(&fixture_path(), &sig, PUBKEY).expect("签名与产物对得上");
        assert!(!FIXTURE.is_empty());

        // 同一段内容被改一个字节就必须验不过：拿临时文件演一次
        let tampered = std::env::temp_dir().join("sideshift-verify-tampered.bin");
        let mut bytes = FIXTURE.to_vec();
        bytes[0] = bytes[0].wrapping_add(1);
        std::fs::write(&tampered, &bytes).unwrap();
        let err = verify_file_with(&tampered, &sig, PUBKEY).unwrap_err();
        let _ = std::fs::remove_file(&tampered);
        assert_eq!(err, app_code("update-sig-mismatch"));
    }

    /// 没内置公钥时**根本不该走到验签那一步**，且报的是「缺钥」而不是「验不过」——
    /// 这一档就是 §4 那条降级态的另一半：P1 这一期还没接密钥，界面上说的是我们没配好
    #[test]
    fn no_builtin_key_blocks_before_verification_not_as_a_failure() {
        let sig = decode_signature(SIG).unwrap();
        assert_eq!(verify_file(&fixture_path(), &sig), Err(app_code("update-key-missing")));
    }

    /// 编码可以放宽、内容不能：四行明文形态与一行 base64 形态解出同一个签名
    #[test]
    fn both_sig_encodings_decode_to_the_same_signature() {
        let one_line = decode_signature(SIG).unwrap();
        let block = base64::engine::general_purpose::STANDARD.decode(SIG.trim()).unwrap();
        let four_line = decode_signature(&String::from_utf8(block).unwrap()).unwrap();
        let key = decode_public_key(PUBKEY).unwrap();
        for sig in [&one_line, &four_line] {
            assert!(key.verify_stream(sig).is_ok());
            assert_eq!(sig.untrusted_comment(), "untrusted comment: signature from tauri secret key");
        }
    }

    /// 公钥的三种可达形态都解得开，而且都能验过同一份产物：
    /// 一行 base64 的两行块（官方 `.pub`/`plugins.updater.pubkey`）、两行明文、42 字节裸钥的 base64。
    /// 内置那一枚将来直接抄官方那一行，配置格式不该逼着改代码。
    ///
    /// 比对走「验得过」而不是 `assert_eq!(key, key)`：`PublicKey::decode` 会把 untrusted comment
    /// 存进结构体，`from_base64` 存的是 `None`，两种形态的钥**按 PartialEq 本来就不相等**
    /// （0.3.0 实测），而我们要的等价是「同一个 Ed25519 钥」，那只有验签能证明
    #[test]
    fn every_public_key_encoding_verifies_the_same_bytes() {
        let sig = decode_signature(SIG).unwrap();
        let four_line = String::from_utf8(b64(PUBKEY.trim()).unwrap()).unwrap();
        let bare = four_line.lines().last().unwrap();
        for raw in [PUBKEY, four_line.as_str(), bare] {
            verify_file_with(&fixture_path(), &sig, raw).expect("三种公钥写法都该验得过");
        }
    }

    /// 一把认不出的钥（不是 42 字节、也不是块）⇒ 种类码，不回 minisign 那句英文
    #[test]
    fn a_malformed_public_key_gets_a_code() {
        let want = Some(app_code("update-key-invalid"));
        assert_eq!(decode_public_key("not-a-key").err(), want);
        assert_eq!(decode_public_key("").err(), want);
        // 长度不是 42 字节的裸钥（"hello"）
        assert_eq!(decode_public_key("aGVsbG8=").err(), want);
    }

    /// 不是签名的东西（HTML 错误页、半个文件）解不开 ⇒ 种类码，不把 minisign 那句英文抬上来。
    /// （比 `.err()` 不比整个 `Result`：`Signature` 没有 `PartialEq`/`Debug`，拿不到成功值那一侧）
    #[test]
    fn garbage_sig_text_gets_a_code_not_an_english_message() {
        let want = Some(app_code("update-sig-format"));
        assert_eq!(decode_signature("<html>404</html>").err(), want);
        assert_eq!(decode_signature("not base64 at all 🙃").err(), want);
        assert_eq!(decode_signature("").err(), want);
    }
}

//! 端判定证据采集：判定一个模组「服务端要不要」的取证层，不含裁决规则（裁决在 detector）。
//!
//! 证据阶梯（可信度由高到低，detector 依 `rank` 取第一层命中的）：
//! 1. 包内 jar 的 `fabric.mod.json:environment`（没写则看 `entrypoints` 段）——模组作者自证，
//!    且是加载器运行时真在执行的声明（本模块离线扫描）
//! 2. Modrinth 按文件 sha1 反查构建的 `environment`——平台侧、精确到这一个文件
//!    （裸 zip 的 index 没给哈希 → 本模块扫描时自行计算 jar 字节 sha1）
//! 3. Modrinth 项目级 `client_side/server_side`——按文件名/包内自报 id 推出的 slug，
//!    名字对得上才采信；再兜不住按模组显示名走 `/v2/search`（第 3 层内，同样标平台项目）
//! 4. `mrpack files[].env`——打包者抄的第二手声明，整表无区分度时整层作废（见 detector）
//! 5. detector 的模组名关键字表——纯猜，兜底；jar 里有服务端注册事实时不许它开口
//!
//! 第 3、4 层是网络查询，结果落 `cache_dir/env-index.json`：同一个模组第二次见到即离线可答。
//! Forge / NeoForge 的 `META-INF/mods.toml` 没有自证端字段（实测），所以第 2 层只覆盖 Fabric / Quilt；
//! 那里补一道**字节码结构提示**（`read_code_facts`）：只用来按住名称关键字层的误删与给一句提示，
//! 不参与剔除——实测「引用了哪些 MC 类」区分不了两端，能区分的只有加载器的注册 API。
//! 但 jar 内的 `id` / `displayName` 仍然有价值：国内整合包常把 jar 文件名整体改成中文，
//! 此时文件名推不出任何线索，只有包内自报身份能把行对上 Modrinth 项目。
//!
//! 本文件只是模块根（barrel）：对外仍然用 `crate::core::env::X` 访问，内部按取证层分五块——
//! `evidence`（证据结构与可信度）· `jar`（第 1 层：包内 jar 元数据离线探测）·
//! `code`（字节码结构提示）· `index`（第 3/4 层：Modrinth 反查与本地索引）·
//! `ident`（模组身份 → 项目引用推导）。

mod code;
mod evidence;
mod ident;
mod index;
mod jar;

pub use code::{CodeFacts, CodeMap};
pub use evidence::{rank, Evidence, EvidenceMap};
pub use ident::targets_for;
pub use index::{
    apply_index, apply_probes, resolve_added_build, resolve_local_jar, resolve_online, EnvIndex,
    ONLINE_BUDGET,
};
pub use jar::{probe_jars, probe_local_jar, ProbeReq};

/// 测试夹具：造 zip / class 字节。jar 层与 code 层的用例都要用，放这一处避免各写一份
#[cfg(test)]
pub(crate) mod fixtures {
    use std::io::{Cursor, Write};

    pub fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// 造一个只含常量池的 class：Utf8 串按顺序放进 CP，后面接全零的结构区。
    /// 只需要常量池能被正确走完，字段/方法体都留空
    pub fn class_bytes(strings: &[&str]) -> Vec<u8> {
        let mut b: Vec<u8> = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 0];
        b.extend(((strings.len() + 1) as u16).to_be_bytes());
        for s in strings {
            b.push(1);
            b.extend((s.len() as u16).to_be_bytes());
            b.extend(s.as_bytes());
        }
        b.extend(std::iter::repeat(0u8).take(14));
        b
    }
}

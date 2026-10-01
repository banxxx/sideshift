//! 端判定证据采集：取证一个模组「服务端要不要」，按可信度阶梯（jar 自证 > 平台构建反查 >
//! 平台项目声明 > 镜像项目声明 > 百科词条 > 整合包 files[].env > 名称启发兜底），不含裁决规则（裁决在 detector）。
//! - 第 3/4 层是网络查询，结果落 `cache_dir/env-index.json`，同一模组第二次见到即离线可答。
//! - 百科那一层是补全：只在平台各腿全答不上的行上发请求，两个平台源都跑它（见 `index::resolve_via_mcmod`）。
//! - Forge / NeoForge 元数据没有自证端字段，jar 自证与构建反查只覆盖 Fabric / Quilt；
//!   字节码结构提示（`read_code_facts`）只用来按住名称关键字层的误删，不参与剔除。
//! - 模块根（barrel）：`evidence`·`jar`·`code`·`index`·`ident` 五块，对外用 `crate::core::env::X`。

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

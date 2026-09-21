//! 字节码结构提示：加载器元数据没自证端时的最后一道保险。
//! 它不是证据层级的一员，只在 detector 里当「名称关键字层准不准开口」的闸门。

use std::collections::HashMap;
use std::io::{Cursor, Read};

/// jar 字节的结构事实。用途被刻意限制在「少删」这一侧——13 个真实 Modrinth 包实测：
/// 纯客户端模组照样大量引用 `net/minecraft/world/`（要读实体和方块才渲染得出来），
/// 两端包也照样引用 `net/minecraft/client/`。「引用了哪些 MC 类」根本区分不了两端，
/// 唯一分得开的是**加载器自己的注册 API**（这部分不参与混淆，Forge 侧也一样是明文）：
/// 3 个 client_only 模组（新旧两个版本各测一遍）服务端标记 0 命中，
/// AppleSkin / JEI / Create 全部 ≥2 命中。所以这里只允许两种结论：
/// 「确有服务端注册 → 别靠猜名字删它」与「形状像纯客户端 → 只提示、不删」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodeFacts {
    /// 有只有服务端才会跑的注册（common 生命周期 / 注册表 / 网络 payload / 服务端类）
    pub server_code: bool,
    /// 只见客户端生命周期注册、不见任何服务端注册：形状像纯客户端（不据此剔除）
    pub client_only_shape: bool,
}

/// 包内条目路径 → 字节码结构事实（与 `EvidenceMap` 平行的一条独立通道：
/// 它不是证据层级的一员，只在 detector 里当「名称关键字层准不准开口」的闸门）
pub type CodeMap = HashMap<String, CodeFacts>;

/// 服务端注册的词汇表。刻意不收 `net/minecraft/world/`、`net/minecraftforge/common/`
/// 这类宽口径——纯客户端模组也在用（改包后实测会把 Entity Culling 判成服务端必需）
const SERVER_TOKENS: &[&str] = &[
    "FMLCommonSetupEvent",
    "CommonSetupEvent",
    "RegisterEvent",
    "DeferredRegister",
    "RegisterPayloadHandlersEvent",
    "InterModComms",
    "net/minecraft/server/",
    "ServerAboutToStartEvent",
    "ServerStartingEvent",
    "RegisterCapabilitiesEvent",
    "AddReloadListenerEvent",
    "EntityAttributeCreationEvent",
    "FMLPreInitializationEvent",
    "FMLInitializationEvent",
    "ServerLifecycleEvents",
];

/// 客户端注册的词汇表（只为「形状」提示服务，不参与任何剔除决定）
const CLIENT_TOKENS: &[&str] = &[
    "FMLClientSetupEvent",
    "ClientSetupEvent",
    "RegisterGuiLayersEvent",
    "ClientTickEvent",
    "ClientPlayerNetworkEvent",
    "RegisterParticleProvidersEvent",
    "RenderTickEvent",
];

/// 单个 jar 最多解多少个 class：命中服务端标记即提前收工，所以这条只兜住
/// 「纯客户端的大 jar」那种最坏情况（解 class 是离线层最贵的一步）
const CODE_MAX_CLASSES: usize = 4000;

/// 扫 jar 内所有 class 的常量池，得出结构事实。读不动的条目静默跳过（交回上层）
pub fn read_code_facts(jar: &[u8]) -> CodeFacts {
    let mut facts = CodeFacts::default();
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(jar)) else {
        return facts;
    };
    let mut seen = 0usize;
    let mut client_hit = false;
    for i in 0..archive.len() {
        let Ok(name) = archive.by_index(i).map(|f| f.name().to_string()) else {
            continue;
        };
        if !name.ends_with(".class") || name.starts_with("META-INF/versions/") {
            continue;
        }
        if seen >= CODE_MAX_CLASSES {
            break;
        }
        seen += 1;
        let mut buf = Vec::new();
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        // 找到服务端事实就收工：它的结论（不许删）已经是这轮能给出的最强事实
        if utf8_entries(&buf, &mut |s| {
            if !client_hit && CLIENT_TOKENS.iter().any(|t| s.contains(t)) {
                client_hit = true;
            }
            SERVER_TOKENS.iter().any(|t| s.contains(t))
        }) {
            facts.server_code = true;
            return facts;
        }
    }
    facts.client_only_shape = client_hit;
    facts
}

/// 逐个把 class 常量池里的 Utf8 串交给 `visit`；`visit` 返回 true 时提前收工。
/// 只走常量池、不进字段/方法/属性区：那里曾有属性长度错位的老坑（Python 原型踩过），
/// 而这些标记全都在常量池里，不必冒结构错位的险。实测 7000+ 个 class 零解析失败
fn utf8_entries(b: &[u8], visit: &mut impl FnMut(&str) -> bool) -> bool {
    /// JVMS 4.4：tag → 该条目在 tag 之后占几字节（Long/Double 占两槽，见外层处理）
    const fn size(tag: u8) -> Option<usize> {
        Some(match tag {
            1 => return None, // 变长，外层单独处理
            3 | 4 => 4,
            5 | 6 => 8,
            7 | 8 | 16 | 19 | 20 | 21 => 2,
            9 | 10 | 11 | 12 | 17 | 18 => 4,
            15 => 3,
            _ => return None,
        })
    }
    if b.len() < 10 || b[..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
        return false;
    }
    let count = u16::from_be_bytes([b[8], b[9]]) as usize;
    let mut p = 10usize;
    let mut i = 1usize;
    while i < count {
        let Some(tag) = b.get(p) else { return false };
        p += 1;
        if *tag == 1 {
            let Some(&[l0, l1]) = b.get(p..p + 2) else {
                return false;
            };
            let len = u16::from_be_bytes([l0, l1]) as usize;
            p += 2;
            let Some(text) = b.get(p..p + len).and_then(|s| std::str::from_utf8(s).ok()) else {
                return false;
            };
            p += len;
            if visit(text) {
                return true;
            }
        } else {
            let Some(n) = size(*tag) else { return false };
            p += n;
            // Long / Double 占两个常量池槽位
            if *tag == 5 || *tag == 6 {
                i += 1;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::fixtures::{class_bytes, zip_bytes};

    /// 混合定长条目的 class：Long（占两槽）、MethodHandle（3 字节）、InvokeDynamic（4 字节）
    /// 各一个，再跟一个 Utf8。宽度算错的话尾部会解不出标记
    fn class_bytes_mixed() -> Vec<u8> {
        let mut b: Vec<u8> = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 0];
        b.extend(6u16.to_be_bytes()); // Long 占两槽 → Utf8 落在槽位 5，count 得写到 6
        b.push(5); // Long
        b.extend(std::iter::repeat(0u8).take(8));
        b.push(15); // MethodHandle
        b.extend([1, 0, 2]);
        b.push(18); // InvokeDynamic
        b.extend([0, 3, 0, 4]);
        b.push(1);
        let s = b"Lnet/minecraftforge/fml/event/lifecycle/FMLCommonSetupEvent;";
        b.extend((s.len() as u16).to_be_bytes());
        b.extend(s);
        b.extend(std::iter::repeat(0u8).take(14));
        b
    }

    fn jar_of(classes: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let refs: Vec<(&str, &[u8])> = classes
            .iter()
            .map(|(n, b)| (*n, b.as_slice()))
            .collect();
        zip_bytes(&refs)
    }

    #[test]
    fn code_facts_flag_server_registration_but_not_rendering_code() {
        let server = jar_of(&[(
            "com/x/Setup.class",
            class_bytes(&[
                "com/x/Setup",
                "Lnet/minecraftforge/fml/event/lifecycle/FMLCommonSetupEvent;",
            ]),
        )]);
        let f = read_code_facts(&server);
        assert!(f.server_code);
        assert!(!f.client_only_shape, "有服务端注册时不该再标纯客户端形状");

        // 纯客户端模组照样大量引用 world/entity 类：这类前缀不参与判定
        let client_only_mod = jar_of(&[(
            "com/x/Hud.class",
            class_bytes(&[
                "com/x/Hud",
                "net/minecraft/world/entity/LivingEntity",
                "net/minecraft/client/gui/GuiGraphics",
            ]),
        )]);
        let f = read_code_facts(&client_only_mod);
        assert!(!f.server_code);
        assert!(!f.client_only_shape, "只引用了 MC 类、没有任何注册 → 形状也不下结论");

        // 只见客户端生命周期注册 → 记为「形状像纯客户端」
        let client_setup = jar_of(&[(
            "com/x/Client.class",
            class_bytes(&[
                "com/x/Client",
                "Lnet/minecraftforge/fml/client/event/FMLClientSetupEvent;",
            ]),
        )]);
        let f = read_code_facts(&client_setup);
        assert!(!f.server_code);
        assert!(f.client_only_shape);
    }

    #[test]
    fn utf8_walker_survives_variable_slot_constant_pool_entries() {
        let jar = jar_of(&[("com/x/Mixed.class", class_bytes_mixed())]);
        assert!(read_code_facts(&jar).server_code);
    }

    #[test]
    fn code_facts_stop_at_versioned_classes_and_survive_garbage() {
        // META-INF/versions/ 下的多版本副本不代表这个 jar 的注册行为
        let jar = jar_of(&[
            ("com/x/Main.class", class_bytes(&["com/x/Main"])),
            (
                "META-INF/versions/21/com/x/Main.class",
                class_bytes(&["com/x/Main", "RegisterCapabilitiesEvent"]),
            ),
        ]);
        let f = read_code_facts(&jar);
        assert!(!f.server_code);
        // 非 class 条目与坏字节：静默无结论，不 panic
        assert_eq!(read_code_facts(&zip_bytes(&[("a.txt", b"hi".as_slice())])), f);
        assert_eq!(read_code_facts(b"not a zip at all"), CodeFacts::default());
        let mut truncated = class_bytes(&["java/lang/Object", "DeferredRegister"]);
        truncated.truncate(20);
        assert!(
            !read_code_facts(&jar_of(&[("com/x/T.class", truncated)])).server_code,
            "半截 class 不能解出错位结论"
        );
    }
}

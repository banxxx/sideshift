//! MC 版本号的「版本线」解析：老编号 `1.20.5` 与新年份号 `26.1.2` 在一个出口收口。
//!
//! 老编号的版本线藏在第二段（`1.20.5` → 20，`1.` 只是固定前缀），26 起的年份号就写在首段
//! （`26.1.2` → 26）。只读 `split('.')[1]` 的写法在年份号下会把 `26.3` 读成 3 线，判出来的
//! 档位当场错一整档（Java 需求线、属性文件编码），所以凡按版本线比较的地方都走这里。

/// 一条版本号拆出来的两段整数：版本线 + 补丁号（第三段，缺则为 0）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line {
    pub line: u32,
    pub patch: u32,
}

/// 认不出形状的返回 `None`，由调用方决定兜底档位。
///
/// 刻意**不**从 `24w14a` 里抠出 24：那串是周编号，跟版本线不同源，抠出来会把一个
/// 老快照判成未来版本。同理整段严格转整数——`6-fabric` 不当 6 读。
pub fn parse(mc: &str) -> Option<Line> {
    // 老 Beta/Alpha 带字母前缀（`b1.7.3`、`a1.0.15_02`），先剥掉再按数字读
    let head = mc
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let mut seg = head.split('.');
    let first = number(seg.next())?;
    let (line, patch) = if first == 1 {
        (number(seg.next())?, number(seg.next()).unwrap_or_default())
    } else {
        (first, number(seg.next()).unwrap_or_default())
    };
    Some(Line { line, patch })
}

fn number(s: Option<&str>) -> Option<u32> {
    s?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_numbering_schemes_land_on_their_own_version_line() {
        let cases = [
            ("1.20.5", Some((20, 5))),
            ("1.21.11", Some((21, 11))),
            ("1.20", Some((20, 0))),
            ("26.3", Some((26, 3))),
            // 年份号也带第三段（26.1.1 / 26.1.2 实测在册）
            ("26.1.2", Some((26, 1))),
            // 字母前缀的老版本
            ("b1.7.3", Some((7, 3))),
            ("a1.0.15_02", Some((0, 0))),
        ];
        for (mc, want) in cases {
            assert_eq!(parse(mc).map(|l| (l.line, l.patch)), want, "{mc}");
        }
    }

    /// 抠数字的捷径一律不许走通：认不出就是认不出，兜底档位由调用方定
    #[test]
    fn refuses_everything_that_is_not_a_dotted_version() {
        for mc in ["", "   ", "24w14a", "latest", "1", "1.x"] {
            assert_eq!(parse(mc), None, "{mc}");
        }
    }

    /// 段末带杂字的按「缺补丁号」处理，而不是把 `6` 从 `6-fabric` 里抠出来
    #[test]
    fn a_dirty_patch_segment_reads_as_no_patch_at_all() {
        assert_eq!(parse("1.20.6-fabric").map(|l| (l.line, l.patch)), Some((20, 0)));
    }
}

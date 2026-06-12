//! MC 版本 → 所需 Java major 的映射。
//!
//! 数据来源：Mojang 公开的 client.json（"javaVersion.majorVersion"）+ Paper 启动器
//! 自身的版本要求。这张表只覆盖 1.16.5 起的 Paper 支持范围；未知版本回退到
//! 当前已知的最大值（保守策略，宁可装个新 JRE 也别启不起来）。

/// 给定 MC 版本号字符串（"1.20.4"、"1.21"、"26.1.2" 等），返回最低要求的 Java major。
///
/// 解析失败 / 未知版本 → 返回最高已知值（21）。
pub fn required_java_major(mc_version: &str) -> u32 {
    // 新版本号格式（26.x.x）：Mojang 2026 年起改用年号制，全部需要 Java 21+。
    if !mc_version.starts_with("1.") {
        return 21;
    }

    let parsed = parse_mc_version(mc_version);
    let (_major, minor, patch) = match parsed {
        Some(t) => t,
        None => return 21,
    };

    match (minor, patch) {
        (m, _) if m >= 21 => 21,
        (20, p) if p >= 5 => 21,
        (20, _) => 17,
        (18..=19, _) => 17,
        (17, _) => 16,
        (16, _) => 8,
        _ => 8,
    }
}

/// 把 "1.20.4" 拆成 (1, 20, 4)。"1.21" → (1, 21, 0)。失败返回 None。
fn parse_mc_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let mut it = s.split('.');
    let major: u32 = it.next()?.parse().ok()?;
    let minor: u32 = it.next()?.parse().ok()?;
    let patch_raw = it.next().unwrap_or("0");
    // 1.21 / 1.21-pre1 / 1.20.4-snapshot 都要切到第一个非数字。
    let patch_num: String = patch_raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    let patch: u32 = if patch_num.is_empty() { 0 } else { patch_num.parse().ok()? };
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_8_for_old() {
        assert_eq!(required_java_major("1.16.5"), 8);
    }
    #[test]
    fn java_17_branch() {
        assert_eq!(required_java_major("1.18.2"), 17);
        assert_eq!(required_java_major("1.19.4"), 17);
        assert_eq!(required_java_major("1.20.1"), 17);
        assert_eq!(required_java_major("1.20.4"), 17);
    }
    #[test]
    fn java_21_branch() {
        assert_eq!(required_java_major("1.20.5"), 21);
        assert_eq!(required_java_major("1.20.6"), 21);
        assert_eq!(required_java_major("1.21"), 21);
        assert_eq!(required_java_major("1.21.4"), 21);
    }
    #[test]
    fn unknown_falls_back_to_max() {
        assert_eq!(required_java_major("1.99.0"), 21);
        assert_eq!(required_java_major("garbage"), 21);
    }
    #[test]
    fn snapshot_prefix_handled() {
        assert_eq!(required_java_major("1.20.4-pre1"), 17);
    }
    #[test]
    fn new_version_scheme_26x() {
        assert_eq!(required_java_major("26.1"), 21);
        assert_eq!(required_java_major("26.1.2"), 21);
        assert_eq!(required_java_major("26.2"), 21);
        assert_eq!(required_java_major("27.0.1"), 21);
    }
}

//! Java runtime 探测。优先级：
//!   1. `JAVA_HOME/bin/java`
//!   2. `PATH` 上的 `java`
//!   3. 调用方再额外补 "managed JDK 缓存" 候选
//!
//! 设计参考 `herald-launcher::java`（已在 Herald 项目中验证）。
//!
//! 不调 java 真启动就没办法可靠拿到 majorVersion —— 解析 `-version` stderr 是
//! Mojang launcher 都在用的方法。第一次扫描会有一次进程启动开销，后续可缓存。

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize)]
pub struct JavaInfo {
    pub path: PathBuf,
    pub major: u32,
    pub vendor: Option<String>,
    /// 用户可读的来源标签：JAVA_HOME / PATH / managed / config。
    pub source: &'static str,
}

/// 扫 JAVA_HOME 与 PATH。再加上 `extra` 中的候选目录（`<dir>/bin/java[.exe]`）—— 
/// 用于把 managed JDK 缓存目录传进来一起列。
///
/// 重复路径自动去重。
pub fn probe(extra_dirs: &[PathBuf]) -> Vec<JavaInfo> {
    let mut out = Vec::new();

    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = bin_in(&PathBuf::from(home));
        if let Some(info) = inspect(&p, "JAVA_HOME") {
            out.push(info);
        }
    }
    if let Ok(p) = which::which("java") {
        if let Some(info) = inspect(&p, "PATH") {
            out.push(info);
        }
    }
    for dir in extra_dirs {
        let p = bin_in(dir);
        if let Some(info) = inspect(&p, "managed") {
            out.push(info);
        }
    }

    let mut seen = std::collections::HashSet::new();
    out.retain(|j| seen.insert(j.path.canonicalize().unwrap_or_else(|_| j.path.clone())));
    out
}

/// 选满足 `>= required` 的；在满足者里挑 major 最小的（最稳）。
pub fn pick_for(required_major: u32, extra_dirs: &[PathBuf]) -> Option<JavaInfo> {
    let mut all = probe(extra_dirs);
    all.retain(|j| j.major >= required_major);
    all.sort_by_key(|j| j.major);
    all.into_iter().next()
}

fn bin_in(dir: &Path) -> PathBuf {
    dir.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" })
}

pub fn inspect(java: &Path, source: &'static str) -> Option<JavaInfo> {
    if !java.exists() {
        return None;
    }
    let out = Command::new(java).arg("-version").output().ok()?;
    let s = String::from_utf8_lossy(&out.stderr).to_string();
    let major = parse_major(&s)?;
    let vendor = if s.contains("Temurin") {
        Some("Eclipse Temurin".into())
    } else if s.contains("OpenJDK") {
        Some("OpenJDK".into())
    } else if s.contains("HotSpot") {
        Some("Oracle HotSpot".into())
    } else if s.contains("GraalVM") {
        Some("GraalVM".into())
    } else {
        None
    };
    Some(JavaInfo {
        path: java.to_path_buf(),
        major,
        vendor,
        source,
    })
}

fn parse_major(s: &str) -> Option<u32> {
    let i = s.find("version \"")?;
    let rest = &s[i + 9..];
    let end = rest.find('"')?;
    let v = &rest[..end];
    if v.starts_with("1.") {
        v.split('.').nth(1)?.parse().ok()
    } else {
        v.split('.').next()?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modern() {
        assert_eq!(parse_major("openjdk version \"21.0.1\" 2023-10-17"), Some(21));
    }
    #[test]
    fn parse_legacy() {
        assert_eq!(parse_major("java version \"1.8.0_341\""), Some(8));
    }
    #[test]
    fn parse_with_extra() {
        assert_eq!(
            parse_major("OpenJDK 64-Bit Server VM\njava version \"17.0.5\" 2022-10-18"),
            Some(17)
        );
    }
}

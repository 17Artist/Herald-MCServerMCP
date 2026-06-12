//! 路径沙箱：把"用户/AI 给的相对路径"安全地解析到一个固定根之下，禁绝
//! `..` 越界与符号链接逃逸。
//!
//! 用法：
//! ```ignore
//! let safe = sandbox::resolve(work_dir, user_path)?;
//! // safe 一定在 work_dir 之下
//! ```

use std::path::{Component, Path, PathBuf};

use crate::error::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("路径不能为空")]
    Empty,
    #[error("不允许使用绝对路径")]
    Absolute,
    #[error("路径越界（含 .. 或符号链接逃逸）")]
    Escape,
    #[error("文件名不合法（包含 NUL 等控制字符）")]
    BadName,
}

impl From<SandboxError> for ApiError {
    fn from(e: SandboxError) -> Self {
        ApiError::bad_request("invalid_path", e.to_string())
    }
}

/// 给定 root + 用户输入路径（相对），返回 root 之下的绝对路径。
/// 不要求目标存在；存在时会顺便 canonicalize 检查 symlink 逃逸。
pub fn resolve(root: &Path, user_path: &str) -> Result<PathBuf, SandboxError> {
    let user_path = user_path.trim();
    if user_path.is_empty() {
        return Err(SandboxError::Empty);
    }
    if user_path.contains('\0') {
        return Err(SandboxError::BadName);
    }
    let p = Path::new(user_path);
    if p.is_absolute() {
        return Err(SandboxError::Absolute);
    }

    // 拼接前先把组件规范化（剔除 CurDir，遇到 ParentDir 视为越界）。
    let mut out = PathBuf::from(root);
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SandboxError::Escape);
            }
        }
    }

    // 无论目标文件是否存在，对其父目录做 canonicalize 检查。
    // 这防止了：文件不存在时，父目录是 symlink/junction 指向 root 外部的逃逸。
    let check_dir = if out.exists() {
        out.clone()
    } else {
        out.parent().unwrap_or(&out).to_path_buf()
    };
    if check_dir.exists() {
        let canonical = check_dir.canonicalize().map_err(|_| SandboxError::Escape)?;
        let root_canonical = root.canonicalize().map_err(|_| SandboxError::Escape)?;
        if !canonical.starts_with(&root_canonical) {
            return Err(SandboxError::Escape);
        }
    }
    Ok(out)
}

/// 严格的"单层文件名"约束：用户提交的 plugin jar 名必须只是文件名（无目录分隔），
/// 后缀小写为 `.jar`，长度合理。返回干净的文件名（去掉路径段）。
pub fn validate_jar_filename(name: &str) -> Result<String, SandboxError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SandboxError::Empty);
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(SandboxError::BadName);
    }
    if name.starts_with('.') {
        return Err(SandboxError::BadName);
    }
    if name.len() > 200 {
        return Err(SandboxError::BadName);
    }
    if !name.to_ascii_lowercase().ends_with(".jar") {
        return Err(SandboxError::BadName);
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        let root = std::env::temp_dir();
        assert!(matches!(resolve(&root, "../etc/passwd"), Err(SandboxError::Escape)));
        assert!(matches!(resolve(&root, "a/../b"), Err(SandboxError::Escape)));
    }

    #[test]
    fn rejects_absolute() {
        let root = std::env::temp_dir();
        let abs = if cfg!(windows) { "C:\\Windows" } else { "/etc/passwd" };
        assert!(matches!(resolve(&root, abs), Err(SandboxError::Absolute)));
    }

    #[test]
    fn accepts_clean_relative() {
        let root = std::env::temp_dir();
        let r = resolve(&root, "plugins/EssentialsX.jar").unwrap();
        assert!(r.starts_with(&root));
    }

    #[test]
    fn validate_jar_name() {
        assert!(validate_jar_filename("EssentialsX.jar").is_ok());
        assert!(validate_jar_filename("my-plugin-1.0.0.jar").is_ok());
        assert!(validate_jar_filename("../evil.jar").is_err());
        assert!(validate_jar_filename("hidden/.jar").is_err());
        assert!(validate_jar_filename("notjar.txt").is_err());
        assert!(validate_jar_filename("").is_err());
    }
}

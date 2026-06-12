//! 数据目录与子路径解析。
//!
//! 优先用 `data_dir` 配置项（见 `config.toml`）。配置未给则回退到：
//!   * Linux:   `$XDG_DATA_HOME/herald-mcserver`  或 `~/.local/share/herald-mcserver`
//!   * Windows: `%APPDATA%\HeraldMcServer`
//!   * macOS:   `~/Library/Application Support/dev.heralders.HeraldMcServer`
//!
//! 调用方应只用本模块拼路径，避免散落 `data_dir` 字面量。

use std::path::{Path, PathBuf};

/// 计算最终的 data_dir：配置里给了就用配置（相对路径转绝对），否则平台默认。
pub fn resolve_data_dir(configured: Option<&str>) -> PathBuf {
    if let Some(s) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(s);
        return if p.is_absolute() {
            p
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(p)
        };
    }
    if let Some(dirs) = directories::ProjectDirs::from("dev", "heralders", "HeraldMcServer") {
        return dirs.data_dir().to_path_buf();
    }
    PathBuf::from("./data")
}

pub fn auth_db(data_dir: &Path) -> PathBuf {
    data_dir.join("auth.db")
}

pub fn server_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("server")
}

pub fn runtimes_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("runtimes")
}

pub fn jdk_cache_root(data_dir: &Path) -> PathBuf {
    runtimes_dir(data_dir).join("jdk")
}

pub fn paper_cache(data_dir: &Path) -> PathBuf {
    runtimes_dir(data_dir).join("paper")
}

/// `setup.lock` —— 防止并发首次注册（owner 创建期间不允许第二次提交）。
pub fn setup_lock(data_dir: &Path) -> PathBuf {
    data_dir.join("setup.lock")
}

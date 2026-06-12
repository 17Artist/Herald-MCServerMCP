//! TOML 配置加载。结构跟 `config.example.toml` 一一对应。
//!
//! 全部字段都给默认值，所以一份空 toml 也能跑（用于开发）。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub mc: McConfig,
    pub runtime: RuntimeConfig,
    pub security: SecurityConfig,
    pub mcp: McpConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            mc: McConfig::default(),
            runtime: RuntimeConfig::default(),
            security: SecurityConfig::default(),
            mcp: McpConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
    /// 监听地址，例如 `0.0.0.0:8787`。
    pub listen: String,
    /// 仅用于面板里"复制连接 URL"的展示，不参与路由。
    pub public_url: String,
    /// 外网 IP 或域名（告诉 AI 客户端"用什么地址连 MC 服务器"）。
    /// 留空则 `mc_env_probe` 返回 `listen` 的 IP 部分。
    pub public_host: String,
    /// 数据目录。空字符串 → 走平台默认 (`paths::resolve_data_dir`)。
    pub data_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8787".into(),
            public_url: String::new(),
            public_host: String::new(),
            data_dir: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct McConfig {
    pub default_version: String,
    pub heap_mb: u32,
    /// MC 游戏端口。每次启动 Paper 都会写入 server.properties。
    pub server_port: u16,
    /// 留空则由"环境管家"在启动 server 时自动选/下载。
    pub java_path: String,
    /// 留空则启动 Paper 前随机生成并写 server.properties。
    pub rcon_password: String,
    pub rcon_port: u16,
}

impl Default for McConfig {
    fn default() -> Self {
        Self {
            default_version: "1.21.11".into(),
            heap_mb: 4096,
            server_port: 25565,
            java_path: String::new(),
            rcon_password: String::new(),
            rcon_port: 25575,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub auto_install_java: bool,
    pub auto_install_paper: bool,
    /// "default" / "tuna" / "bmclapi" / 直接给 base URL。
    pub mirror: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            auto_install_java: true,
            auto_install_paper: true,
            mirror: "default".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub session_ttl_secs: i64,
    /// owner 之外是否允许 `/api/auth/signup`。默认 false（仅邀请）。
    pub allow_signup: bool,
    /// 信任的反向代理 IP 段（CIDR 列表，逗号分隔）。配上之后才读 `X-Forwarded-For`。
    pub trusted_proxy: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            session_ttl_secs: 30 * 24 * 3600,
            allow_signup: false,
            trusted_proxy: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct McpConfig {
    pub enabled: bool,
    pub require_api_key: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_api_key: true,
        }
    }
}

impl Config {
    /// 从 toml 路径加载；文件缺失返回 `ConfigNotFound`，调用方决定是否回退到 `Config::default()`。
    pub fn load_from(path: &Path) -> Result<Self, CoreError> {
        if !path.exists() {
            return Err(CoreError::ConfigNotFound(path.display().to_string()));
        }
        let text = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// 加载或返回默认值。**仅用于开发** —— 生产部署请显式指定 config 路径以免静默走默认。
    pub fn load_or_default(path: &Path) -> Self {
        match Self::load_from(path) {
            Ok(c) => c,
            Err(CoreError::ConfigNotFound(_)) => {
                tracing::warn!("config {} not found, using defaults", path.display());
                Self::default()
            }
            Err(e) => {
                tracing::error!("config load failed: {e}, using defaults");
                Self::default()
            }
        }
    }

    fn validate(&self) -> Result<(), CoreError> {
        if self.server.listen.is_empty() {
            return Err(CoreError::InvalidConfig("server.listen empty".into()));
        }
        if self.mc.heap_mb < 256 {
            return Err(CoreError::InvalidConfig(
                "mc.heap_mb too small (>=256 required)".into(),
            ));
        }
        Ok(())
    }
}

//! Herald-MCServerMCP 共享层。
//!
//! 这里只放：
//!   * 配置加载/保存 (`config`)
//!   * 数据目录路径解析 (`paths`)
//!   * 跨 crate 共用的错误类型 (`error`)
//!
//! 业务模块（认证、进程监管、HTTP 路由）都不放在这里。

pub mod config;
pub mod error;
pub mod paths;

pub use config::{Config, McConfig, McpConfig, RuntimeConfig, SecurityConfig, ServerConfig};
pub use error::CoreError;

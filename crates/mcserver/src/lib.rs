//! Herald-MCServerMCP — Paper 子进程监管。
//!
//! 模块：
//!   * [`process`]  — `tokio::process::Command` 包装 + 日志泵
//!   * [`events`]   — broadcast 到 WebSocket 的事件类型
//!   * [`instance`] — 全局唯一的 [`ServerInstance`]，承载状态机
//!
//! 调用方：crates/server 在 AppState 里持有一个 `ServerInstance`，handler 走它。

pub mod events;
pub mod instance;
pub mod process;

pub use events::{LogLine, ServerEvent};
pub use instance::{RconEndpoint, ServerInstance, StartError, StartErrorWire, StartOptions};
pub use process::{ServerSnapshot, ServerStatus};

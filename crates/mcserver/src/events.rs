//! 服务端事件类型。这些 enum 既是内部 broadcast 的 payload，也直接序列化到
//! WebSocket（前端拿到的 JSON 就是这个 enum 的 untagged 表示）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub ts: i64,
    pub stream: &'static str, // "stdout" / "stderr"
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// 状态机翻面：Stopped → Starting → Running → Stopping → Stopped。
    StatusChange { status: crate::process::ServerStatus, pid: Option<u32> },
    /// 一行日志。
    Log { line: LogLine },
}

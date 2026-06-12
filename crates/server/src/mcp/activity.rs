//! MCP 调用活动总线。
//!
//! 服务端每次处理 `tools/call` 都广播一对事件（start + finish），WebSocket 把它们
//! 推到浏览器，前端用动效条渲染——让"人类"能直观看到 AI 正在通过 MCP 控这台服务器。
//!
//! 事件分两种：
//!   * Start  —— 工具调用开始（payload 含 tool 名 / 关键参数摘要 / api key 名）
//!   * Finish —— 调用结束（status: ok | error | forbidden，duration_ms）
//!
//! 设计取舍：
//!   * 不保留任意 args 全文（可能含 plugin jar base64、文件内容等长串）；
//!     只截 < 200 字符的 summary 字段。
//!   * 用 `id` 在 start/finish 之间做关联，前端据此做"进行中 → 完成"动画。

use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpActivity {
    Start {
        id: String,
        tool: String,
        /// args 摘要（<= 200 chars）。
        summary: String,
        /// API key 名称（不含 secret）。
        key_name: String,
        scope: &'static str,
        ts: i64,
    },
    Finish {
        id: String,
        tool: String,
        status: ActivityStatus,
        /// 失败时的简短错误说明。
        message: Option<String>,
        duration_ms: u64,
        ts: i64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    Ok,
    Error,
    Forbidden,
}

#[derive(Clone)]
pub struct ActivityBus {
    tx: broadcast::Sender<McpActivity>,
    /// 最近 N 条历史，让浏览器刷新后能直接拿到时间线。
    history: Arc<RwLock<Vec<McpActivity>>>,
}

const HISTORY_CAP: usize = 100;

impl Default for ActivityBus {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tx,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<McpActivity> {
        self.tx.subscribe()
    }

    pub fn publish(&self, ev: McpActivity) {
        {
            let mut g = self.history.write().expect("activity history poisoned");
            g.push(ev.clone());
            if g.len() > HISTORY_CAP {
                let drop_n = g.len() - HISTORY_CAP;
                g.drain(0..drop_n);
            }
        }
        let _ = self.tx.send(ev);
    }

    pub fn history(&self) -> Vec<McpActivity> {
        self.history.read().expect("activity history poisoned").clone()
    }
}

pub fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as i64
}

/// 把 args JSON 压成一行简短摘要给前端展示（最多 ~200 char）。
///
/// 长字段（content_b64 / content / token / password / secret 等）替换成
/// `<XX bytes>` 占位，不暴露内容；其它原样保留。
pub fn summarize_args(args: &serde_json::Value) -> String {
    let mut s = match args {
        serde_json::Value::Object(map) => {
            let mut parts: Vec<String> = Vec::new();
            for (k, v) in map {
                let lower = k.to_ascii_lowercase();
                let elide = matches!(
                    lower.as_str(),
                    "content"
                        | "content_text"
                        | "content_b64"
                        | "secret"
                        | "password"
                        | "token"
                );
                let formatted = if elide {
                    let n = match v {
                        serde_json::Value::String(s) => s.len(),
                        _ => v.to_string().len(),
                    };
                    format!("<{n} bytes>")
                } else {
                    short_value(v)
                };
                parts.push(format!("{k}={formatted}"));
            }
            parts.join(", ")
        }
        other => short_value(other),
    };
    if s.chars().count() > 200 {
        let mut t = String::new();
        let mut count = 0;
        for c in s.chars() {
            if count >= 197 {
                break;
            }
            t.push(c);
            count += 1;
        }
        t.push_str("...");
        s = t;
    }
    s
}

fn short_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            if s.chars().count() > 60 {
                let mut t: String = s.chars().take(57).collect();
                t.push_str("...");
                format!("\"{t}\"")
            } else {
                format!("\"{s}\"")
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            let raw = v.to_string();
            if raw.len() > 60 {
                format!("{}...", &raw[..57])
            } else {
                raw
            }
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn elides_long_content() {
        let s = summarize_args(&json!({
            "path": "server.properties",
            "content": "x".repeat(8000),
        }));
        assert!(s.contains("path=\"server.properties\""));
        assert!(s.contains("content=<8000 bytes>"));
    }

    #[test]
    fn truncates_long_summary() {
        // 构造多键使得拼接后 > 200 chars（每个键 short_value 截到 60，需 4+ 键）
        let s = summarize_args(&json!({
            "aaa": "x".repeat(100),
            "bbb": "y".repeat(100),
            "ccc": "z".repeat(100),
            "ddd": "w".repeat(100),
        }));
        assert!(s.chars().count() <= 200, "actual len: {}", s.chars().count());
        assert!(s.ends_with("..."), "actual suffix: ...{}", &s[s.len().saturating_sub(10)..]);
    }

    #[test]
    fn shortens_long_strings_per_field() {
        let s = summarize_args(&json!({ "filename": "x".repeat(120) }));
        assert!(s.contains("..."));
        assert!(!s.contains(&"x".repeat(120)));
    }
}

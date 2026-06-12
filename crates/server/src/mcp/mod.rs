//! MCP Streamable HTTP transport（2025-03-26 spec 简化版）。
//!
//! 端点：`POST /mcp`，body 是 JSON-RPC 2.0 单条请求，返回 `application/json`
//! 单条响应。我们暂不实现 SSE 长连（spec 允许"server may respond with text/event-stream"，
//! 但纯请求/响应模式对 stateless tool call 已经够用）。
//!
//! 鉴权：`Authorization: Bearer mck_...`（API Key），通过 [`ApiKeyUser`] extractor。
//! 调用方如果是只读 scope，调写工具会被 422 / forbidden 顶回去。
//!
//! 协议子集：
//!   * initialize  — 协议握手，返回 capabilities + serverInfo
//!   * tools/list  — 工具发现
//!   * tools/call  — 调用具体工具（args 经过每个工具的 schema 验证）
//!
//! 我们不实现 prompts、resources、logging —— 不在调试闭环工具的范畴。
//!
//! 旁路：每次 `tools/call` 会发一对 [`activity::McpActivity`] 事件到 ActivityBus，
//! WebSocket 把它们推到浏览器 → 浏览器渲染"AI 正在调用 xxx 工具"动效。

pub mod activity;
mod tools;

use axum::{
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{middleware::auth::ApiKeyUser, state::AppState};

pub use activity::{ActivityBus, ActivityStatus, McpActivity};
pub use tools::ToolName;

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc")]
    jsonrpc: String,
    /// id 可以是 string / number / null（通知）。我们直接用 Value 透传回去。
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

fn default_jsonrpc() -> String {
    "2.0".to_string()
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// 标准 JSON-RPC 错误码 + MCP 自有扩展。
mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    /// 工具执行失败（业务错误）。
    pub const TOOL_ERROR: i32 = -32000;
}

pub async fn mcp_handler(
    Extension(state): Extension<AppState>,
    user: ApiKeyUser,
    body: String,
) -> Response {
    if !state.config.mcp.enabled {
        return error_response(
            Value::Null,
            codes::INVALID_REQUEST,
            "MCP 接口已被服务端禁用（config.toml [mcp].enabled=false）",
            StatusCode::SERVICE_UNAVAILABLE,
        );
    }

    // Rate limit: 单 key 每 60s 最多 120 次调用（包含 ping/list/call）。
    // 走全工具的正常 AI 流程一次可能 5-20 次，远低于这个阈值；恶意脚本会被卡。
    let key_id = format!("mcp_key:{}", user.key.id);
    if !state
        .rate_limit
        .check(&key_id, 120, std::time::Duration::from_secs(60))
    {
        return error_response(
            Value::Null,
            codes::TOOL_ERROR,
            "MCP 调用频率超限（每 60s 最多 120 次）。请放慢节奏或升级配额。",
            StatusCode::TOO_MANY_REQUESTS,
        );
    }

    let req: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                Value::Null,
                codes::PARSE_ERROR,
                format!("解析 JSON-RPC 请求失败: {e}"),
                StatusCode::BAD_REQUEST,
            );
        }
    };

    if req.jsonrpc != "2.0" {
        return error_response(
            req.id,
            codes::INVALID_REQUEST,
            "jsonrpc 字段必须是 \"2.0\"",
            StatusCode::BAD_REQUEST,
        );
    }

    let id = req.id.clone();
    let result = dispatch(state.clone(), &user, &req).await;
    match result {
        Ok(v) => Json(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(v),
            error: None,
        })
        .into_response(),
        Err(e) => error_response(
            id,
            e.code,
            e.message,
            // 业务错误统一返回 200 + JSON-RPC error（client 看 result/error 区分）。
            // 协议层错误（解析失败等）才用 4xx。
            if e.code == codes::PARSE_ERROR
                || e.code == codes::INVALID_REQUEST
                || e.code == codes::METHOD_NOT_FOUND
                || e.code == codes::INVALID_PARAMS
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            },
        ),
    }
}

fn error_response(id: Value, code: i32, msg: impl Into<String>, status: StatusCode) -> Response {
    let body = JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: msg.into(),
            data: None,
        }),
    };
    (status, Json(body)).into_response()
}

#[derive(Debug)]
struct DispatchError {
    code: i32,
    message: String,
}

impl DispatchError {
    fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: codes::INVALID_PARAMS,
            message: msg.into(),
        }
    }
    fn tool(msg: impl Into<String>) -> Self {
        Self {
            code: codes::TOOL_ERROR,
            message: msg.into(),
        }
    }
    fn forbidden(msg: impl Into<String>) -> Self {
        Self {
            code: codes::TOOL_ERROR,
            message: msg.into(),
        }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: codes::INTERNAL_ERROR,
            message: msg.into(),
        }
    }
}

async fn dispatch(
    state: AppState,
    user: &ApiKeyUser,
    req: &JsonRpcRequest,
) -> Result<Value, DispatchError> {
    match req.method.as_str() {
        "initialize" => Ok(initialize_result()),

        "notifications/initialized" => {
            // 通知；按 spec 返回空 result。client 一般不会等响应。
            Ok(json!({}))
        }

        "ping" => Ok(json!({})),

        "tools/list" => Ok(json!({ "tools": tools::tool_list_json() })),

        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DispatchError::invalid_params("params.name 必须是字符串"))?;
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));

            let tool_name = ToolName::parse(name).ok_or_else(|| {
                DispatchError {
                    code: codes::METHOD_NOT_FOUND,
                    message: format!("未知工具: {name}"),
                }
            })?;

            // 发 Start 事件 —— 让浏览器知道 AI 正在调
            let activity_id = Uuid::new_v4().simple().to_string()[..12].to_string();
            let started_at = activity::now_millis();
            let summary = activity::summarize_args(&args);
            state.mcp_activity.publish(McpActivity::Start {
                id: activity_id.clone(),
                tool: name.to_string(),
                summary,
                key_name: user.key.name.clone(),
                scope: user.key.scope.as_str(),
                ts: started_at,
            });

            // scope 校验：所有写工具需要 mcp:full
            if tool_name.requires_full_scope()
                && !user
                    .key
                    .scope
                    .covers(herald_mcserver_auth::ApiKeyScope::McpFull)
            {
                let elapsed = (activity::now_millis() - started_at).max(0) as u64;
                state.mcp_activity.publish(McpActivity::Finish {
                    id: activity_id,
                    tool: name.to_string(),
                    status: ActivityStatus::Forbidden,
                    message: Some(format!("需要 scope: mcp:full（当前 mcp:read）")),
                    duration_ms: elapsed,
                    ts: activity::now_millis(),
                });
                return Err(DispatchError::forbidden(format!(
                    "工具 {name} 需要 scope: mcp:full（当前 key 仅有 mcp:read）"
                )));
            }

            let result = tools::call_tool(state.clone(), tool_name, args).await;
            let elapsed = (activity::now_millis() - started_at).max(0) as u64;
            let (status, message) = match &result {
                Ok(_) => (ActivityStatus::Ok, None),
                Err(e) => (
                    ActivityStatus::Error,
                    Some(short_error_message(&e.message)),
                ),
            };
            state.mcp_activity.publish(McpActivity::Finish {
                id: activity_id,
                tool: name.to_string(),
                status,
                message,
                duration_ms: elapsed,
                ts: activity::now_millis(),
            });
            result
        }

        _ => Err(DispatchError {
            code: codes::METHOD_NOT_FOUND,
            message: format!("未知方法: {}", req.method),
        }),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false },
        },
        "serverInfo": {
            "name": "herald-mcserver",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Minecraft 插件调试沙盒：可用工具组 env_*/server_*/plugin_*/files_*/rcon_*。先调 mc_env_probe 看 Java/Paper 状态；缺则触发 mc_env_install_*；齐了直接 mc_server_start 启动。",
    })
}

/// 从 DispatchError.message（可能是嵌套 JSON）里截一段 ≤ 120 字符的简短描述。
fn short_error_message(raw: &str) -> String {
    // 如果是 JSON，尝试抽 summary 字段
    if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
        if let Some(s) = parsed.get("summary").and_then(|v| v.as_str()) {
            return truncate(s, 120);
        }
        if let Some(s) = parsed.get("message").and_then(|v| v.as_str()) {
            return truncate(s, 120);
        }
    }
    truncate(raw, 120)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

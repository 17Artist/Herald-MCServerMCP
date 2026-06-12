//! `/api/activity/*` —— 浏览器拉 MCP 调用历史。
//!
//! 实时事件走 WebSocket（channel: "mcp_activity"）。这个 HTTP 端点只用于
//! 页面打开时拉一下历史时间线（broadcast 不保留）。

use axum::{extract::Extension, Json};

use crate::{middleware::auth::SessionUser, state::AppState};

pub async fn list(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Json<Vec<crate::mcp::McpActivity>> {
    Json(s.mcp_activity.history())
}

pub fn router() -> axum::Router {
    use axum::routing::get;
    axum::Router::new().route("/list", get(list))
}

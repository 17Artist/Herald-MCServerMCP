//! Router 组装。把所有 `/api/*` 路由 + WebSocket + MCP + SPA fallback 拼到一起。

use std::sync::Arc;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Extension, Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::trace::TraceLayer;

use crate::{mcp, routes, state::AppStateInner, static_assets, ws};

pub fn build(state: AppStateInner) -> Router {
    let state = Arc::new(state);

    let api = Router::new()
        .nest("/setup", routes::setup::router())
        .nest("/auth", routes::auth::router())
        .nest("/env", routes::env::router())
        .nest("/server", routes::server::router())
        .nest("/plugins", routes::plugins::router())
        .nest("/files", routes::files::router())
        .nest("/rcon", routes::rcon::router())
        .nest("/keys", routes::api_keys::router())
        .nest("/activity", routes::activity::router())
        .nest("/admin", routes::admin::router());

    Router::new()
        .nest("/api", api)
        .route("/ws", get(ws::ws_handler))
        .route("/mcp", post(mcp::mcp_handler))
        .fallback(static_assets::spa_handler)
        // 全局 body 上限 100 MiB（覆盖 MCP plugin_upload base64 + multipart plugin upload）
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(CookieManagerLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(Extension(state))
}

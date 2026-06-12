//! `/api/keys/*` —— API Key 面板后端。
//!
//! 路由：
//!   GET    /api/keys/list      列出当前用户的 key（不含 secret）
//!   POST   /api/keys/create    创建一把 key —— **secret 仅在响应里露一次**
//!   DELETE /api/keys/{id}      吊销一把 key（软删除：写 revoked_at）
//!
//! 鉴权：cookie session（同 owner / member 都可以管自己创建的 key）。
//! Owner 才能看到 `mcp_endpoint`（其实任何人都能看；仅是面板的便利字段）。

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
};

#[derive(Serialize)]
pub struct KeyDto {
    pub id: String,
    pub name: String,
    pub scope: &'static str,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

impl From<herald_mcserver_auth::ApiKey> for KeyDto {
    fn from(k: herald_mcserver_auth::ApiKey) -> Self {
        Self {
            id: k.id,
            name: k.name,
            scope: k.scope.as_str(),
            created_at: k.created_at,
            last_used_at: k.last_used_at,
            revoked_at: k.revoked_at,
        }
    }
}

pub async fn list(
    Extension(s): Extension<AppState>,
    SessionUser { user, .. }: SessionUser,
) -> Result<Json<Vec<KeyDto>>, ApiError> {
    let keys = s.auth.list_api_keys(&user.id)?;
    Ok(Json(keys.into_iter().map(KeyDto::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateReq {
    pub name: String,
    /// "mcp:full" / "mcp:read"
    pub scope: String,
}

#[derive(Serialize)]
pub struct CreateResp {
    pub key: KeyDto,
    /// 明文 secret，**只露这一次**。
    pub secret: String,
}

pub async fn create(
    Extension(s): Extension<AppState>,
    SessionUser { user, .. }: SessionUser,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<CreateResp>), ApiError> {
    let scope = herald_mcserver_auth::ApiKeyScope::parse(req.scope.trim())
        .ok_or_else(|| ApiError::bad_request("invalid_scope", "scope 必须是 mcp:full 或 mcp:read"))?;
    let created = s.auth.create_api_key(&user.id, &req.name, scope)?;
    Ok((
        StatusCode::CREATED,
        Json(CreateResp {
            key: KeyDto::from(created.key),
            secret: created.secret,
        }),
    ))
}

pub async fn revoke(
    Extension(s): Extension<AppState>,
    SessionUser { user, .. }: SessionUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.auth.revoke_api_key(&user.id, &id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct EndpointResp {
    pub mcp_url: String,
    pub mcp_enabled: bool,
}

/// 面板用：返回完整 MCP 接入 URL（基于 config.public_url 或 server.listen 推断）。
pub async fn endpoint(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Json<EndpointResp> {
    let base = if !s.config.server.public_url.is_empty() {
        s.config.server.public_url.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", s.config.server.listen)
    };
    Json(EndpointResp {
        mcp_url: format!("{base}/mcp"),
        mcp_enabled: s.config.mcp.enabled,
    })
}

pub fn router() -> axum::Router {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route("/list", get(list))
        .route("/create", post(create))
        .route("/endpoint", get(endpoint))
        .route("/:id", delete(revoke))
}

//! 认证中间件 + extractor。
//!
//! 设计：
//!   * Cookie 名 `mcs_session`，HttpOnly + SameSite=Lax + Secure（生产）
//!   * Bearer token（API Key）走 `Authorization: Bearer mck_...`
//!
//! Extractor：
//!   * `AuthUser` —— session 或 API Key 任意一种通过即放行
//!   * `SessionUser` —— 仅 cookie session
//!   * `OwnerUser` —— SessionUser + role==Owner
//!   * `ApiKeyUser` —— 仅 API Key

use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
    Extension,
};
use herald_mcserver_auth::{ApiKey, ApiKeyScope, Role, User};
use tower_cookies::Cookies;

use crate::{error::ApiError, state::AppState};

pub const SESSION_COOKIE: &str = "mcs_session";

/// Cookie session 校验得到的用户。
#[allow(dead_code)] // token 字段在 S5 audit 时使用
pub struct SessionUser {
    pub user: User,
    pub token: String,
}

/// 带 Owner 限定 —— 用于管理员路由（S5 启用）。
#[allow(dead_code)]
pub struct OwnerUser(pub User);

/// API Key 校验得到的凭证（S4 启用）。
#[allow(dead_code)]
pub struct ApiKeyUser {
    pub key: ApiKey,
    pub user: User,
}

impl ApiKeyUser {
    #[allow(dead_code)] // S4 调用方启用
    pub fn require_scope(&self, needed: ApiKeyScope) -> Result<(), ApiError> {
        if self.key.scope.covers(needed) {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "api_key_scope",
                format!("此操作需要 scope: {}", needed.as_str()),
            ))
        }
    }
}

/// 任意凭证（session 或 API Key）。MCP 客户端走 Key，浏览器走 cookie（S4 启用）。
#[allow(dead_code)]
pub enum AuthUser {
    Session(SessionUser),
    ApiKey(ApiKeyUser),
}

#[allow(dead_code)]
impl AuthUser {
    pub fn user(&self) -> &User {
        match self {
            AuthUser::Session(s) => &s.user,
            AuthUser::ApiKey(k) => &k.user,
        }
    }
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for SessionUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let Extension(state): Extension<AppState> = Extension::from_request_parts(parts, &())
            .await
            .map_err(|_| ApiError::internal("missing AppState extension"))?;

        let cookies = Cookies::from_request_parts(parts, &())
            .await
            .map_err(|_| ApiError::internal("cookie layer missing"))?;

        let token = cookies
            .get(SESSION_COOKIE)
            .ok_or_else(|| ApiError::unauthorized("session_required", "未登录"))?
            .value()
            .to_string();

        let validated = state
            .auth
            .validate_session_token(&token)
            .map_err(ApiError::from)?;

        Ok(SessionUser {
            user: validated.user,
            token,
        })
    }
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for OwnerUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, st: &S) -> Result<Self, Self::Rejection> {
        let s = SessionUser::from_request_parts(parts, st).await?;
        if s.user.role != Role::Owner {
            return Err(ApiError::forbidden("owner_required", "需要管理员权限"));
        }
        Ok(OwnerUser(s.user))
    }
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for ApiKeyUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let Extension(state): Extension<AppState> = Extension::from_request_parts(parts, &())
            .await
            .map_err(|_| ApiError::internal("missing AppState extension"))?;

        let header_val = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("api_key_required", "缺少 Authorization 头"))?;

        let token = header_val
            .strip_prefix("Bearer ")
            .or_else(|| header_val.strip_prefix("bearer "))
            .ok_or_else(|| {
                ApiError::unauthorized("api_key_required", "Authorization 必须是 Bearer 形式")
            })?;

        let (key, user) = state
            .auth
            .validate_api_key(token.trim())
            .map_err(ApiError::from)?;
        Ok(ApiKeyUser { key, user })
    }
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, st: &S) -> Result<Self, Self::Rejection> {
        // Bearer 优先 —— 如果带了 Authorization 头就走 API Key 链路；
        // 没带再尝试 cookie session。
        if parts.headers.contains_key(header::AUTHORIZATION) {
            return ApiKeyUser::from_request_parts(parts, st)
                .await
                .map(AuthUser::ApiKey);
        }
        SessionUser::from_request_parts(parts, st)
            .await
            .map(AuthUser::Session)
    }
}

/// 用于诊断：让 handler 能拿到 Arc<AppStateInner> 而不必走 Extension trait。
#[allow(dead_code)]
pub fn state_from_parts(parts: &Parts) -> Option<AppState> {
    parts.extensions.get::<AppState>().cloned().or_else(|| {
        parts
            .extensions
            .get::<Arc<crate::state::AppStateInner>>()
            .cloned()
    })
}

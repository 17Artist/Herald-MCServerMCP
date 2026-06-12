//! HTTP 错误统一封装。
//!
//! Handler 的返回类型用 `Result<T, ApiError>`；ApiError 实现了 `IntoResponse`，
//! 自动序列化成 `{ error: "code", message: "..." }` 的 JSON 体。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use herald_mcserver_auth::AuthError;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(code: &'static str, msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, msg)
    }

    pub fn unauthorized(code: &'static str, msg: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, msg)
    }

    pub fn forbidden(code: &'static str, msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, msg)
    }

    #[allow(dead_code)] // 留给 S2+ 路由使用
    pub fn not_found(code: &'static str, msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, msg)
    }

    pub fn gone(code: &'static str, msg: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, code, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "error": self.code,
            "message": self.message,
        }));
        (self.status, body).into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::InvalidUsername { reason } => {
                ApiError::bad_request("invalid_username", reason)
            }
            AuthError::InvalidPassword { reason } => {
                ApiError::bad_request("invalid_password", reason)
            }
            AuthError::OwnerExists => ApiError::gone("owner_exists", "管理员账户已存在"),
            AuthError::WrongPassword | AuthError::UserNotFound => {
                ApiError::unauthorized("invalid_credentials", "用户名或密码错误")
            }
            AuthError::AccountLocked { until } => ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "account_locked",
                format!("账户已被锁定，解锁时间 {until}"),
            ),
            AuthError::SessionInvalid => {
                ApiError::unauthorized("session_invalid", "会话失效，请重新登录")
            }
            AuthError::ApiKeyInvalid => {
                ApiError::unauthorized("api_key_invalid", "API Key 无效或已吊销")
            }
            AuthError::ApiKeyScope { scope } => ApiError::forbidden(
                "api_key_scope",
                format!("此操作需要 scope: {scope}"),
            ),
            other => ApiError::internal(other.to_string()),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::internal(e.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::internal(e.to_string())
    }
}

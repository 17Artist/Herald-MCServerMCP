//! `/api/auth/*` —— 登录、登出、当前会话查询。

use axum::{
    extract::{ConnectInfo, Extension},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use tower_cookies::{Cookie, Cookies};

use crate::{
    error::ApiError,
    middleware::auth::{SessionUser, SESSION_COOKIE},
    state::AppState,
    util::rate_limit::client_ip_key,
};

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
    /// 浏览器侧可填一个稳定的设备 hint（localStorage UUID），用来"同设备同账户"挤掉旧 session。
    #[serde(default)]
    pub device: Option<String>,
}

#[derive(Serialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub role: String,
}

pub async fn login(
    Extension(s): Extension<AppState>,
    cookies: Cookies,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<Json<UserDto>, ApiError> {
    // Rate limit: 同 IP 每 5 分钟最多 20 次 login，防止暴力枚举
    let trust_xff = !s.config.security.trusted_proxy.is_empty();
    let ip = client_ip_key(&headers, Some(addr.ip()), trust_xff);
    let key = format!("login:{ip}");
    if !s
        .rate_limit
        .check(&key, 20, std::time::Duration::from_secs(300))
    {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "登录请求过于频繁，请稍后重试",
        ));
    }

    let ttl = s.config.security.session_ttl_secs;
    let result = s.auth.login(
        &req.username,
        &req.password,
        req.device.as_deref(),
        Some(ttl),
    );
    let (user, session) = match result {
        Ok(t) => {
            s.auth.audit(herald_mcserver_auth::NewAudit {
                kind: "user.login",
                actor_id: Some(&t.0.id),
                actor_label: Some(&t.0.username),
                ok: true,
                detail: Some(&format!("ip={ip}")),
            });
            t
        }
        Err(e) => {
            s.auth.audit(herald_mcserver_auth::NewAudit {
                kind: "user.login",
                actor_id: None,
                actor_label: Some(&req.username),
                ok: false,
                detail: Some(&format!("ip={ip} err={e}")),
            });
            return Err(e.into());
        }
    };

    let mut cookie = Cookie::new(SESSION_COOKIE, session.token);
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
    if s.config.server.public_url.starts_with("https://") {
        cookie.set_secure(true);
    }
    cookies.add(cookie);

    Ok(Json(UserDto {
        id: user.id,
        username: user.username,
        role: user.role.as_str().to_string(),
    }))
}

pub async fn logout(
    Extension(s): Extension<AppState>,
    cookies: Cookies,
) -> Result<StatusCode, ApiError> {
    if let Some(c) = cookies.get(SESSION_COOKIE) {
        let _ = s.auth.revoke_session(c.value());
    }
    let mut clear = Cookie::new(SESSION_COOKIE, "");
    clear.set_path("/");
    cookies.remove(clear);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(SessionUser { user, .. }: SessionUser) -> Json<UserDto> {
    Json(UserDto {
        id: user.id,
        username: user.username,
        role: user.role.as_str().to_string(),
    })
}

#[derive(Deserialize)]
pub struct RedeemReq {
    pub code: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub device: Option<String>,
}

/// 公开端点：用 owner 给的邀请码注册一个 member 账号。
/// 成功直接颁发 session（与 setup init 同款"注册即登录"）。
pub async fn redeem(
    Extension(s): Extension<AppState>,
    cookies: Cookies,
    Json(req): Json<RedeemReq>,
) -> Result<Json<UserDto>, ApiError> {
    let ttl = s.config.security.session_ttl_secs;
    let (user, session) = s
        .auth
        .redeem_invite_register(
            req.code.trim(),
            &req.username,
            &req.password,
            req.device.as_deref(),
            Some(ttl),
        )?;

    let mut cookie = Cookie::new(SESSION_COOKIE, session.token);
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
    if s.config.server.public_url.starts_with("https://") {
        cookie.set_secure(true);
    }
    cookies.add(cookie);

    s.auth.audit(herald_mcserver_auth::NewAudit {
        kind: "user.register",
        actor_id: Some(&user.id),
        actor_label: Some(&user.username),
        ok: true,
        detail: Some(&format!("via invite {}", req.code.trim())),
    });

    Ok(Json(UserDto {
        id: user.id,
        username: user.username,
        role: user.role.as_str().to_string(),
    }))
}

pub fn router() -> axum::Router {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route("/login", post(login))
        .route("/redeem", post(redeem))
        .route("/session", delete(logout))
        .route("/me", get(me))
}

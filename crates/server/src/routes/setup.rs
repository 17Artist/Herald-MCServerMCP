//! `/api/setup/*` —— 首次启动向导。
//!
//! 流程：
//!   1. `GET  /api/setup/state`  → `{ initialized: bool }`
//!   2. `POST /api/setup/init`   → 如果尚未初始化，创建 owner，写 setup.lock，
//!                                 同时下发 session cookie 让浏览器直接进控制台
//!
//! 安全：
//!   * `init` 之前先 acquire 文件锁防止并发提交
//!   * DB 真相为准：判断 `auth.owner_exists()`；setup.lock 仅做并发屏障
//!   * 已初始化 → 410 Gone（路径不再可用）

use axum::{extract::Extension, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_cookies::{Cookie, Cookies};

use crate::{
    error::ApiError,
    middleware::auth::SESSION_COOKIE,
    state::AppState,
};

#[derive(Serialize)]
pub struct StateResp {
    pub initialized: bool,
}

pub async fn state(Extension(s): Extension<AppState>) -> Json<StateResp> {
    Json(StateResp {
        initialized: s.is_initialized(),
    })
}

#[derive(Deserialize)]
pub struct InitReq {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct InitResp {
    pub user_id: String,
    pub username: String,
    pub role: String,
}

pub async fn init(
    Extension(s): Extension<AppState>,
    cookies: Cookies,
    Json(req): Json<InitReq>,
) -> Result<(StatusCode, Json<InitResp>), ApiError> {
    // 已初始化 → 410。注意必须用 DB 而不是 setup.lock，因为 lock 可能被手动删了。
    if s.is_initialized() {
        return Err(ApiError::gone("already_initialized", "服务已完成初始化"));
    }

    // setup.lock 用于并发屏障：第一个写入 lock 的请求胜出。
    if let Some(parent) = s.setup_lock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock_result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&s.setup_lock);

    if let Err(e) = lock_result {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "setup_in_progress",
                "另一个客户端正在完成初始化",
            ));
        }
        return Err(e.into());
    }

    // 真正注册 owner。失败要回滚 lock 文件，否则下次进不来。
    let registered = s.auth.register_owner(&req.username, &req.password);
    let user = match registered {
        Ok(u) => u,
        Err(e) => {
            let _ = std::fs::remove_file(&s.setup_lock);
            return Err(e.into());
        }
    };

    // 立即下发 session 让前端无需再走一次 login。
    let ttl = s.config.security.session_ttl_secs;
    let session = s
        .auth
        .issue_session(&user, Some("setup-flow"), Some(ttl))?;

    let mut cookie = Cookie::new(SESSION_COOKIE, session.token);
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
    // Secure 只在 https 下加。开发期 plain http 走不通，因此读 public_url。
    if s.config.server.public_url.starts_with("https://") {
        cookie.set_secure(true);
    }
    cookies.add(cookie);

    Ok((
        StatusCode::CREATED,
        Json(InitResp {
            user_id: user.id,
            username: user.username,
            role: user.role.as_str().to_string(),
        }),
    ))
}

/// 把所有 `/api/setup/*` 装到 router 里。已经初始化也仍然挂着 —— `init` 自己
/// 会返回 410；`state` 永远公开（前端要靠它决定路由跳转）。
pub fn router() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/state", get(state))
        .route("/init", post(init))
        .fallback(|| async {
            Json(json!({"error": "not_found", "message": "未知 setup 路由"}))
        })
}

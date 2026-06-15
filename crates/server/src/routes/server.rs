//! `/api/server/*` —— Paper 进程监管 REST 接口。

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
};

pub async fn status(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Json<herald_mcserver_mcserver::ServerSnapshot> {
    Json(s.server.snapshot())
}

#[derive(Deserialize, Default)]
pub struct StartReq {
    /// 不填 → 用 config.mc.default_version。
    #[serde(default)]
    pub mc_version: Option<String>,
    #[serde(default)]
    pub heap_mb: Option<u32>,
    #[serde(default)]
    pub server_port: Option<u16>,
    #[serde(default)]
    pub rcon_port: Option<u16>,
    #[serde(default)]
    pub rcon_password: Option<String>,
    /// 等 Done 日志的最大秒数，默认 120。
    #[serde(default)]
    pub wait_ready_secs: Option<u64>,
}

pub async fn start(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    body: Option<Json<StartReq>>,
) -> Result<Json<herald_mcserver_mcserver::ServerSnapshot>, ApiError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();
    let opts = build_start_options(&s, req);
    match s.server.start(opts).await {
        Ok(snap) => Ok(Json(snap)),
        Err(e) => Err(map_start_err(e)),
    }
}

#[derive(Deserialize, Default)]
pub struct StopReq {
    #[serde(default)]
    pub force: bool,
}

pub async fn stop(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    body: Option<Json<StopReq>>,
) -> Result<Json<herald_mcserver_mcserver::ServerSnapshot>, ApiError> {
    let force = body.map(|Json(r)| r.force).unwrap_or(false);
    s.server.stop(force).await.map_err(ApiError::from)?;
    // 等 watcher 把状态改回 Stopped；最多等 5s。
    for _ in 0..50 {
        let snap = s.server.snapshot();
        if snap.status == herald_mcserver_mcserver::ServerStatus::Stopped {
            return Ok(Json(snap));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(Json(s.server.snapshot()))
}

pub async fn restart(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    body: Option<Json<StartReq>>,
) -> Result<Json<herald_mcserver_mcserver::ServerSnapshot>, ApiError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();
    let opts = build_start_options(&s, req);
    s.server
        .restart(opts)
        .await
        .map(Json)
        .map_err(map_start_err)
}

#[derive(serde::Serialize)]
pub struct LogsResp {
    pub lines: Vec<herald_mcserver_mcserver::LogLine>,
}

#[derive(Deserialize)]
pub struct LogsQ {
    #[serde(default = "default_tail")]
    pub tail: usize,
}
fn default_tail() -> usize { 200 }

pub async fn logs(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    axum::extract::Query(q): axum::extract::Query<LogsQ>,
) -> Json<LogsResp> {
    Json(LogsResp {
        lines: s.server.tail_logs(q.tail.min(5000)),
    })
}

#[derive(Deserialize)]
pub struct ExecReq {
    pub command: String,
}

pub async fn exec(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Json(req): Json<ExecReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.command.trim().is_empty() {
        return Err(ApiError::bad_request("empty_command", "命令不能为空"));
    }
    s.server
        .send_console(req.command.trim())
        .map_err(|e| ApiError::bad_request("send_failed", e.to_string()))?;
    Ok(axum::http::StatusCode::ACCEPTED)
}

fn build_start_options(
    s: &AppState,
    req: StartReq,
) -> herald_mcserver_mcserver::StartOptions {
    let java_path = {
        let p = s.config.mc.java_path.trim();
        if p.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(p))
        }
    };
    herald_mcserver_mcserver::StartOptions {
        mc_version: req
            .mc_version
            .unwrap_or_else(|| s.config.mc.default_version.clone()),
        heap_mb: req.heap_mb.unwrap_or(s.config.mc.heap_mb),
        server_port: req.server_port.or(Some(s.config.mc.server_port)),
        rcon_port: req.rcon_port.or(Some(s.config.mc.rcon_port)),
        rcon_password: req.rcon_password.or_else(|| {
            let pw = s.config.mc.rcon_password.clone();
            if pw.is_empty() { None } else { Some(pw) }
        }),
        wait_ready_secs: req.wait_ready_secs.unwrap_or(0),
        java_path,
    }
}

fn map_start_err(e: herald_mcserver_mcserver::StartError) -> ApiError {
    use herald_mcserver_mcserver::StartError::*;
    let wire = herald_mcserver_mcserver::StartErrorWire::from(&e);
    let body = serde_json::to_string(&wire).unwrap_or_else(|_| e.to_string());
    let (status, code) = match e {
        EnvMissing { .. } => (axum::http::StatusCode::PRECONDITION_FAILED, "env_missing"),
        BadState { .. } => (axum::http::StatusCode::CONFLICT, "bad_state"),
        ReadyTimeout(_) => (axum::http::StatusCode::REQUEST_TIMEOUT, "ready_timeout"),
        Spawn(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "spawn_failed"),
    };
    ApiError::new(status, code, body)
}

pub fn router() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/status", get(status))
        .route("/start", post(start))
        .route("/stop", post(stop))
        .route("/restart", post(restart))
        .route("/logs", get(logs))
        .route("/exec", post(exec))
}

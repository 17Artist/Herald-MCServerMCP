//! `/api/rcon/*` —— 通过 RCON 协议发命令（短回包）。
//!
//! 与 `/api/server/exec` 的区别：
//!   * `exec` 走子进程 stdin —— 没法拿到 console 回显（那条命令的输出也是日志的一部分），
//!     调用方需要再 GET /logs 才能拿到结果。
//!   * `rcon/exec` 走 RCON TCP —— 直接拿到回包文本，调用方一次到位。
//!
//! 目前每次 exec 都新建一条连接（建连+鉴权 ~5ms 本机）。S4 的 MCP 工具如果调用频繁，
//! 后续考虑改成长连池。

use axum::{extract::Extension, Json};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
};

#[derive(Serialize)]
pub struct EndpointResp {
    pub configured: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// 仅 owner 才会拿到 password 明文（这里 S3 暂时全部返回，S5 再加 role 限制）。
    pub password: Option<String>,
}

pub async fn endpoint(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Json<EndpointResp> {
    match s.server.rcon_endpoint() {
        Some(e) => Json(EndpointResp {
            configured: true,
            host: Some(e.host),
            port: Some(e.port),
            password: Some(e.password),
        }),
        None => Json(EndpointResp {
            configured: false,
            host: None,
            port: None,
            password: None,
        }),
    }
}

#[derive(Deserialize)]
pub struct ExecReq {
    pub command: String,
}

#[derive(Serialize)]
pub struct ExecResp {
    pub command: String,
    pub response: String,
}

pub async fn exec(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Json(req): Json<ExecReq>,
) -> Result<Json<ExecResp>, ApiError> {
    let cmd = req.command.trim().to_string();
    if cmd.is_empty() {
        return Err(ApiError::bad_request("empty_command", "命令不能为空"));
    }
    if cmd.len() > herald_mcserver_rcon::MAX_PAYLOAD {
        return Err(ApiError::bad_request(
            "too_long",
            format!("命令超过 {} 字节上限", herald_mcserver_rcon::MAX_PAYLOAD),
        ));
    }

    let endpoint = s
        .server
        .rcon_endpoint()
        .ok_or_else(|| ApiError::bad_request("server_not_running", "RCON 端点尚未就绪（服务未运行）"))?;

    let addr = format!("{}:{}", endpoint.host, endpoint.port);
    let mut client = herald_mcserver_rcon::RconClient::connect(&addr, &endpoint.password)
        .await
        .map_err(|e| match e {
            herald_mcserver_rcon::RconError::AuthFailed => {
                ApiError::internal("RCON 鉴权失败（密码漂移？请重启服务）")
            }
            herald_mcserver_rcon::RconError::Timeout => {
                ApiError::internal("RCON 连接超时（服务可能仍在启动）")
            }
            other => ApiError::internal(format!("RCON 连接失败: {other}")),
        })?;

    let response = client.exec(&cmd).await.map_err(|e| {
        ApiError::bad_request("exec_failed", format!("RCON 命令执行失败: {e}"))
    })?;

    Ok(Json(ExecResp { command: cmd, response }))
}

pub fn router() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/endpoint", get(endpoint))
        .route("/exec", post(exec))
}

//! `/ws` —— 前端订阅服务端事件 + 任务进度。
//!
//! 鉴权：cookie session（query 参数 token 形式留给 S4 给 MCP 用）。
//! 协议：服务端向 client 推 JSON 文本帧；client → server 仅作 ping/keepalive。

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension,
    },
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::middleware::auth::SessionUser;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(tag = "channel", rename_all = "snake_case")]
enum WsFrame {
    Hello {
        user: String,
        role: String,
    },
    /// 服务端进程事件（启停/日志）。`event` 内层带 `type` 字段。
    Server {
        event: herald_mcserver_mcserver::ServerEvent,
    },
    /// 任务进度事件。
    Task {
        event: herald_mcserver_runtime::TaskEvent,
    },
    /// MCP 调用活动事件 —— 给浏览器渲染"AI 正在控这台服务器"动效用。
    McpActivity {
        event: crate::mcp::McpActivity,
    },
    /// 服务端主动关闭。
    Bye {
        reason: String,
    },
}

pub async fn ws_handler(
    Extension(state): Extension<AppState>,
    user: SessionUser,
    upgrade: WebSocketUpgrade,
) -> Response {
    let username = user.user.username.clone();
    let role = user.user.role.as_str().to_string();
    upgrade.on_upgrade(move |socket| run(socket, state, username, role))
}

async fn run(socket: WebSocket, state: AppState, username: String, role: String) {
    let (mut tx, mut rx) = socket.split();

    if let Err(e) = send_frame(&mut tx, &WsFrame::Hello { user: username.clone(), role }).await {
        tracing::debug!("ws hello failed for {username}: {e}");
        return;
    }

    let mut server_rx = state.server.subscribe();
    let mut task_rx = state.tasks.subscribe();
    let mut mcp_rx = state.mcp_activity.subscribe();

    loop {
        tokio::select! {
            // 服务端事件
            ev = server_rx.recv() => {
                match ev {
                    Ok(e) => {
                        if let Err(err) = send_frame(&mut tx, &WsFrame::Server { event: e }).await {
                            tracing::debug!("ws send server frame failed: {err}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("ws lagged on server bus by {n}");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // 任务进度
            ev = task_rx.recv() => {
                match ev {
                    Ok(e) => {
                        if let Err(err) = send_frame(&mut tx, &WsFrame::Task { event: e }).await {
                            tracing::debug!("ws send task frame failed: {err}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("ws lagged on task bus by {n}");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // MCP 调用活动
            ev = mcp_rx.recv() => {
                match ev {
                    Ok(e) => {
                        if let Err(err) = send_frame(&mut tx, &WsFrame::McpActivity { event: e }).await {
                            tracing::debug!("ws send mcp_activity frame failed: {err}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("ws lagged on mcp_activity bus by {n}");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // 客户端入站（保活/关闭信号）
            incoming = rx.next() => {
                match incoming {
                    Some(Ok(Message::Ping(p))) => {
                        if tx.send(Message::Pong(p)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        tracing::debug!("ws recv error: {e}");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = send_frame(
        &mut tx,
        &WsFrame::Bye {
            reason: "server closing".into(),
        },
    )
    .await;
    let _ = tx.close().await;
}

async fn send_frame<S>(tx: &mut S, frame: &WsFrame) -> Result<(), axum::Error>
where
    S: futures::Sink<Message, Error = axum::Error> + Unpin,
{
    let text = serde_json::to_string(frame).map_err(|e| axum::Error::new(e.to_string()))?;
    tx.send(Message::Text(text)).await
}

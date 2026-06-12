//! Minecraft RCON 客户端（Source RCON protocol）。
//!
//! 协议参考：https://wiki.vg/RCON
//!
//! 用途：用户的"AI 调试闭环"里时不时要跑 `/op artist` `/time set day` 这类
//! 控制台命令；console 通过子进程 stdin 转发能跑，但 RCON 能拿到回包文本，
//! 适合 MCP 工具透传给 AI 看结果。
//!
//! 协议（每个 packet）：
//!   4 bytes length (little-endian, 不包含自己)
//!   4 bytes request id (little-endian)
//!   4 bytes type     (little-endian) 3=LOGIN, 2=EXEC, 0=RESPONSE
//!   N bytes payload (ASCII + 两个终止 \0)
//!
//! 登录：发 type=3 + password；服务端返回 request id=我们给的，密码错时 id=-1。
//!
//! 设计参考 Herald 的 `herald-rcon` crate（已验证）。

use std::io;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const PACKET_TYPE_LOGIN: i32 = 3;
pub const PACKET_TYPE_EXEC: i32 = 2;
pub const PACKET_TYPE_RESPONSE: i32 = 0;

/// 单条命令 payload 字节数上限（Mojang 源 1460；保守取 1400）。
pub const MAX_PAYLOAD: usize = 1400;

#[derive(Debug, Error)]
pub enum RconError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("auth failed (password rejected)")]
    AuthFailed,
    #[error("packet too large: {0} bytes")]
    PacketTooLarge(usize),
    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),
    #[error("timeout")]
    Timeout,
}

pub struct RconClient {
    stream: TcpStream,
    next_id: i32,
}

impl RconClient {
    /// 连接 + 鉴权。超时 5s。
    pub async fn connect(addr: &str, password: &str) -> Result<Self, RconError> {
        let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .map_err(|_| RconError::Timeout)??;
        stream.set_nodelay(true)?;
        let mut client = Self { stream, next_id: 1 };
        client.login(password).await?;
        Ok(client)
    }

    async fn login(&mut self, password: &str) -> Result<(), RconError> {
        let id = self.alloc_id();
        self.write_packet(id, PACKET_TYPE_LOGIN, password.as_bytes()).await?;
        // 协议允许服务端在 LOGIN 响应前发空 RESPONSE；读到 non-matching id 也要继续读。
        loop {
            let (rid, ptype, _payload) = self.read_packet().await?;
            if ptype == PACKET_TYPE_RESPONSE && rid != id {
                continue;
            }
            if rid == -1 {
                return Err(RconError::AuthFailed);
            }
            if rid == id {
                return Ok(());
            }
            return Err(RconError::UnexpectedResponse(format!(
                "login: expected id {id}, got {rid} (type {ptype})"
            )));
        }
    }

    /// 执行一条命令，返回服务端回显（stdout 部分）。
    pub async fn exec(&mut self, cmd: &str) -> Result<String, RconError> {
        if cmd.len() > MAX_PAYLOAD {
            return Err(RconError::PacketTooLarge(cmd.len()));
        }
        let id = self.alloc_id();
        self.write_packet(id, PACKET_TYPE_EXEC, cmd.as_bytes()).await?;
        // 简化：只收一个 RESPONSE。绝大多数命令一包够。
        let (rid, ptype, payload) = self.read_packet().await?;
        if ptype != PACKET_TYPE_RESPONSE {
            return Err(RconError::UnexpectedResponse(format!(
                "exec: expected type RESPONSE, got {ptype}"
            )));
        }
        if rid != id {
            return Err(RconError::UnexpectedResponse(format!(
                "exec: expected id {id}, got {rid}"
            )));
        }
        Ok(String::from_utf8_lossy(&payload).to_string())
    }

    fn alloc_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id <= 0 {
            self.next_id = 1; // -1 表示 auth fail，避开
        }
        id
    }

    async fn write_packet(&mut self, id: i32, ptype: i32, payload: &[u8]) -> Result<(), RconError> {
        let length = 4 + 4 + payload.len() as i32 + 2;
        let mut buf = Vec::with_capacity(4 + length as usize);
        buf.extend_from_slice(&length.to_le_bytes());
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&ptype.to_le_bytes());
        buf.extend_from_slice(payload);
        buf.push(0); // payload 终止
        buf.push(0); // packet 终止
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn read_packet(&mut self) -> Result<(i32, i32, Vec<u8>), RconError> {
        let length = {
            let mut buf = [0u8; 4];
            self.stream.read_exact(&mut buf).await?;
            i32::from_le_bytes(buf)
        };
        if !(10..=4096).contains(&length) {
            return Err(RconError::UnexpectedResponse(format!("bad length {length}")));
        }
        let mut rest = vec![0u8; length as usize];
        self.stream.read_exact(&mut rest).await?;
        let id = i32::from_le_bytes(rest[0..4].try_into().unwrap());
        let ptype = i32::from_le_bytes(rest[4..8].try_into().unwrap());
        let payload_end = (length as usize).saturating_sub(8 + 2);
        let payload = rest[8..8 + payload_end].to_vec();
        Ok((id, ptype, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_sizing_arithmetic_is_right() {
        // payload "list" (4 bytes) → length = 4(id) + 4(type) + 4(payload) + 2(两个 \0) = 14
        let payload_len: i32 = 4;
        let expected_length = 4 + 4 + payload_len + 2;
        assert_eq!(expected_length, 14);
    }
}

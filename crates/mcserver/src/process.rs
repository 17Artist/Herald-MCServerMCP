//! Paper 子进程监管。
//!
//! 状态机：
//!   Stopped → Starting → Running → Stopping → Stopped
//!                  │            └→ (检测到 Done 日志后转 Running)
//!                  └→ Stopped (启动失败)
//!
//! 我们维护一个**全局唯一**的 [`ServerInstance`]：S2 一次只跑一台 Paper。
//! 想换版本就先 stop，再 start 时切 jar。
//!
//! 日志：
//!   * 内存环（最近 5000 行，超出 drain 1000）→ 满足"调试时拉最近日志"
//!   * `tokio::sync::broadcast` → WebSocket 实时订阅
//!
//! Stdin 命令：通过 unbounded mpsc 排队写到子进程 stdin（避免 await 锁）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::events::{LogLine, ServerEvent};

/// 状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerSnapshot {
    pub status: ServerStatus,
    pub pid: Option<u32>,
    pub mc_version: Option<String>,
    pub started_at: Option<i64>,
    pub work_dir: Option<PathBuf>,
}

/// 进程持有 + 日志环。一个 instance 同一时刻最多有一个 Child。
pub struct ServerProcess {
    pub work_dir: PathBuf,
    pub mc_version: String,
    pub pid: u32,
    pub started_at: i64,
    /// child handle 放 Mutex 是为了 stop 时 take 出来 await kill。
    child: Mutex<Option<tokio::process::Child>>,
    /// 排队写 stdin 的发送端。
    stdin_tx: mpsc::UnboundedSender<String>,
    /// 日志环 + broadcast 见 instance 层。
    pub log_ring: Arc<RwLock<Vec<LogLine>>>,
}

impl ServerProcess {
    pub fn send_stdin(&self, cmd: &str) -> anyhow::Result<()> {
        self.stdin_tx
            .send(format!("{cmd}\n"))
            .map_err(|_| anyhow::anyhow!("stdin 通道已关闭"))?;
        Ok(())
    }

    pub async fn kill(&self) {
        if let Some(mut c) = self.child.lock().await.take() {
            let _ = c.kill().await;
        }
    }

    pub async fn wait(&self) -> Option<std::process::ExitStatus> {
        let mut g = self.child.lock().await;
        if let Some(c) = g.as_mut() {
            c.wait().await.ok()
        } else {
            None
        }
    }
}

/// Spawn 选项。
pub struct SpawnOptions<'a> {
    pub java: &'a Path,
    pub jar: &'a Path,
    pub work_dir: &'a Path,
    pub mc_version: &'a str,
    pub heap_mb: u32,
    pub server_port: Option<u16>,
    pub rcon_port: Option<u16>,
    pub rcon_password: Option<&'a str>,
    /// 父进程的事件 bus，子进程的日志会塞进来。
    pub event_tx: broadcast::Sender<ServerEvent>,
    pub log_ring: Arc<RwLock<Vec<LogLine>>>,
}

/// 启动 Paper。返回 (process, ready_rx)。ready_rx 在看到 "Done (...)! For help, type \"help\""
/// 时收到一次信号，调用方据此决定 starting → running 转换。
pub async fn spawn(opts: SpawnOptions<'_>) -> anyhow::Result<(ServerProcess, broadcast::Receiver<()>)> {
    use std::process::Stdio;

    tokio::fs::create_dir_all(opts.work_dir).await?;
    write_initial_files(
        opts.work_dir,
        opts.server_port,
        opts.rcon_port,
        opts.rcon_password,
    )
    .await?;

    let mut cmd = Command::new(opts.java);
    cmd.current_dir(opts.work_dir)
        .arg(format!("-Xmx{}M", opts.heap_mb))
        .arg(format!("-Xms{}M", opts.heap_mb.min(1024)))
        .arg("-jar")
        .arg(opts.jar)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("missing stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("missing stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow::anyhow!("missing stderr"))?;

    let started_at = now_secs();

    // stdin 写入泵
    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(s) = stdin_rx.recv().await {
            if stdin.write_all(s.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
        }
    });

    // ready 信号
    let (ready_tx, ready_rx) = broadcast::channel::<()>(2);

    spawn_log_pump(
        BufReader::new(stdout).lines(),
        false,
        opts.event_tx.clone(),
        opts.log_ring.clone(),
        Some(ready_tx.clone()),
    );
    spawn_log_pump(
        BufReader::new(stderr).lines(),
        true,
        opts.event_tx.clone(),
        opts.log_ring.clone(),
        None,
    );

    Ok((
        ServerProcess {
            work_dir: opts.work_dir.to_path_buf(),
            mc_version: opts.mc_version.to_string(),
            pid,
            started_at,
            child: Mutex::new(Some(child)),
            stdin_tx,
            log_ring: opts.log_ring,
        },
        ready_rx,
    ))
}

fn spawn_log_pump(
    mut lines: tokio::io::Lines<BufReader<impl tokio::io::AsyncRead + Unpin + Send + 'static>>,
    is_stderr: bool,
    event_tx: broadcast::Sender<ServerEvent>,
    log_ring: Arc<RwLock<Vec<LogLine>>>,
    ready_tx: Option<broadcast::Sender<()>>,
) {
    tokio::spawn(async move {
        while let Ok(Some(raw)) = lines.next_line().await {
            let line = LogLine {
                ts: now_millis(),
                stream: if is_stderr { "stderr" } else { "stdout" },
                text: raw,
            };

            // ready 信号（"Done (3.123s)! For help, type ..."）
            if let Some(tx) = &ready_tx {
                if line.text.contains("For help, type") {
                    let _ = tx.send(());
                }
            }

            // 写环
            {
                let mut g = log_ring.write();
                g.push(line.clone());
                if g.len() > 5000 {
                    let drop_n = g.len() - 4000;
                    g.drain(0..drop_n);
                }
            }

            let _ = event_tx.send(ServerEvent::Log { line });
        }
    });
}

async fn write_initial_files(
    work_dir: &Path,
    server_port: Option<u16>,
    rcon_port: Option<u16>,
    rcon_password: Option<&str>,
) -> anyhow::Result<()> {
    // EULA：本工具的使用前提是用户已经接受 Mojang EULA；自动写入 true，避免每次都要手改。
    let eula = work_dir.join("eula.txt");
    if !eula.exists() {
        tokio::fs::write(&eula, "eula=true\n").await?;
    }

    let props = work_dir.join("server.properties");
    let mut body = if props.exists() {
        tokio::fs::read_to_string(&props).await.unwrap_or_default()
    } else {
        String::new()
    };
    // 强制关闭正版验证 —— 这是插件调试工具，否则客户端连不上太麻烦。
    // 每次启动都强制覆盖，防止插件/手动修改后悄悄打开。
    body = patch_property(&body, "online-mode", "false");
    if let Some(p) = server_port {
        body = patch_property(&body, "server-port", &p.to_string());
    }
    if let Some(p) = rcon_port {
        body = patch_property(&body, "enable-rcon", "true");
        body = patch_property(&body, "rcon.port", &p.to_string());
        body = patch_property(&body, "broadcast-rcon-to-ops", "false");
    }
    if let Some(pw) = rcon_password {
        body = patch_property(&body, "rcon.password", pw);
    }
    tokio::fs::write(&props, body).await?;
    Ok(())
}

fn patch_property(body: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}=");
    let mut found = false;
    let mut out: Vec<String> = body
        .lines()
        .map(|l| {
            if l.trim_start().starts_with(&prefix) {
                found = true;
                format!("{key}={value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("{key}={value}"));
    }
    out.join("\n") + "\n"
}

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

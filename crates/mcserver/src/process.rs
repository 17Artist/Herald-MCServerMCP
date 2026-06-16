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
        // 先尝试 tokio 的 Child::kill
        if let Some(mut c) = self.child.lock().await.take() {
            let _ = c.kill().await;
            let _ = c.wait().await; // 等真正退出
            return;
        }
        // Child 已被 take 但进程可能还活——用 OS 级强杀
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &self.pid.to_string()])
                .output();
        }
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill")
                .args(["-9", &self.pid.to_string()])
                .output();
        }
    }

    pub async fn wait(&self) -> Option<std::process::ExitStatus> {
        // stdout=inherit 模式下 tokio child.wait() 可能立即返回。
        // 改用 PID 轮询检测进程是否真的退出。
        loop {
            {
                let mut g = self.child.lock().await;
                if let Some(c) = g.as_mut() {
                    match c.try_wait() {
                        Ok(Some(status)) => return Some(status),
                        Ok(None) => {} // 还在跑
                        Err(_) => return None,
                    }
                } else {
                    return None;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
pub async fn spawn(opts: SpawnOptions<'_>) -> anyhow::Result<(ServerProcess, broadcast::Receiver<()>, tokio::task::AbortHandle)> {
    use std::process::Stdio;

    tokio::fs::create_dir_all(opts.work_dir).await?;
    write_initial_files(
        opts.work_dir,
        opts.server_port,
        opts.rcon_port,
        opts.rcon_password,
    )
    .await?;

    // 直接 spawn java，stdout/stderr 不 pipe（Stdio::inherit），
    // 避免某些插件（如 Blink/Symphony）在 piped stdout 环境下 crash。
    // 日志改从 Paper 的 logs/latest.log 文件轮询读取。
    let mut cmd = Command::new(opts.java);
    cmd.current_dir(opts.work_dir)
        .arg(format!("-Xmx{}M", opts.heap_mb))
        .arg(format!("-Xms{}M", opts.heap_mb.min(1024)))
        .arg("-Djline.terminal=dumb")
        .arg("-jar")
        .arg(opts.jar)
        .arg("nogui")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);

    let mut child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);

    // stdout/stderr 不再 pipe，改用日志文件轮询
    let log_file = opts.work_dir.join("logs").join("latest.log");

    let started_at = now_secs();

    // stdin 写入泵
    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            while let Some(s) = stdin_rx.recv().await {
                if stdin.write_all(s.as_bytes()).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });
    } else {
        tokio::spawn(async move {
            while stdin_rx.recv().await.is_some() {}
        });
    }

    // ready 信号
    let (ready_tx, ready_rx) = broadcast::channel::<()>(2);

    // 日志轮询 + RCON 端口 ready 检测
    let rcon_port = opts.rcon_port.unwrap_or(25575);
    let poller_abort = spawn_log_file_poller(
        log_file,
        rcon_port,
        opts.event_tx.clone(),
        opts.log_ring.clone(),
        Some(ready_tx),
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
        poller_abort,
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

            if let Some(tx) = &ready_tx {
                if line.text.contains("For help, type") {
                    let _ = tx.send(());
                }
            }

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

/// 轮询检测 ready：尝试 TCP 连接 RCON 端口。连通即 ready。
/// 同时持续读 logs/latest.log 填充日志环（best effort，读不到也不影响 ready）。
fn spawn_log_file_poller(
    log_file: std::path::PathBuf,
    rcon_port: u16,
    event_tx: broadcast::Sender<ServerEvent>,
    log_ring: Arc<RwLock<Vec<LogLine>>>,
    ready_tx: Option<broadcast::Sender<()>>,
) -> tokio::task::AbortHandle {
    // RCON 端口 ready 检测（独立任务）
    if let Some(tx) = ready_tx {
        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{rcon_port}");
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                    let _ = tx.send(());
                    break;
                }
            }
        });
    }

    // 日志文件轮询（用字节偏移跟踪，避免 Windows 文件锁导致 read_to_string 读不到新内容）
    let poller_handle = tokio::spawn(async move {
        for _ in 0..120 {
            if log_file.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Paper 启动时会重建 latest.log，从 offset=0 开始读
        let mut offset: u64 = 0;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let log_file_clone = log_file.clone();
            let current_offset = offset;
            let new_content = match tokio::task::spawn_blocking(move || {
                use std::io::{Read, Seek, SeekFrom};
                let mut file = std::fs::File::open(&log_file_clone)?;
                let file_len = file.metadata()?.len();
                // 文件被截断/重建时（Paper 重启会删旧 log）重置 offset
                let actual_offset = if file_len < current_offset { 0 } else { current_offset };
                if file_len <= actual_offset {
                    return Ok::<_, std::io::Error>((String::new(), actual_offset));
                }
                file.seek(SeekFrom::Start(actual_offset))?;
                let to_read = (file_len - actual_offset) as usize;
                let mut buf = vec![0u8; to_read];
                let n = file.read(&mut buf)?;
                buf.truncate(n);
                let new_offset = actual_offset + n as u64;
                Ok((String::from_utf8_lossy(&buf).to_string(), new_offset))
            })
            .await
            {
                Ok(Ok((s, new_off))) => {
                    offset = new_off;
                    s
                }
                _ => continue,
            };

            if new_content.is_empty() {
                continue;
            }

            // 按行推送
            for text in new_content.lines() {
                let text = text.to_string();
                if text.is_empty() {
                    continue;
                }

                let line = LogLine {
                    ts: now_millis(),
                    stream: "stdout",
                    text,
                };

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
        }
    });
    poller_handle.abort_handle()
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

/// 对外暴露 java 检测（给 instance 的 java_path 覆盖逻辑用）。
pub fn inspect_java(path: &std::path::Path) -> Option<herald_mcserver_runtime::JavaInfo> {
    herald_mcserver_runtime::java_probe::inspect(path, "config")
}

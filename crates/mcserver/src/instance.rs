//! 全局唯一的服务端实例封装。
//!
//! S2 阶段假设"一个 herald-mcserver 进程同时只管一个 Paper"。多实例并行留给 v2。
//!
//! [`ServerInstance::start`] 自动：
//!   1. 调用 runtime crate 的 `check_environment` 看 Java/Paper 是否齐
//!   2. 缺则返回结构化错误（`StartError::EnvMissing`），调用方决定要不要触发下载
//!   3. 齐了 → spawn 子进程 → 状态机 Starting → 等"Done!" → Running

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;
use herald_mcserver_runtime::{EnvCheck, Runtime};

use crate::events::{LogLine, ServerEvent};
use crate::process::{spawn, ServerProcess, ServerSnapshot, ServerStatus, SpawnOptions};

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("环境缺失：need Java {need_java_major}; paper jar 缺失？")]
    EnvMissing {
        need_java_major: u32,
        have_java: Option<u32>,
        paper_cached: bool,
    },
    #[error("已经在运行/启动中（status={status:?}）")]
    BadState { status: ServerStatus },
    #[error("启动失败: {0}")]
    Spawn(#[from] anyhow::Error),
    #[error("等待 ready 超时（{0}s）")]
    ReadyTimeout(u64),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "code")]
pub enum StartErrorWire {
    EnvMissing {
        need_java_major: u32,
        have_java: Option<u32>,
        paper_cached: bool,
        suggested_action: &'static str,
    },
    BadState {
        status: ServerStatus,
    },
    Spawn {
        message: String,
    },
    ReadyTimeout {
        seconds: u64,
    },
}

impl From<&StartError> for StartErrorWire {
    fn from(e: &StartError) -> Self {
        match e {
            StartError::EnvMissing {
                need_java_major,
                have_java,
                paper_cached,
            } => StartErrorWire::EnvMissing {
                need_java_major: *need_java_major,
                have_java: *have_java,
                paper_cached: *paper_cached,
                suggested_action: "mc_env_install_java_or_paper",
            },
            StartError::BadState { status } => StartErrorWire::BadState { status: *status },
            StartError::Spawn(e) => StartErrorWire::Spawn {
                message: format!("{e:#}"),
            },
            StartError::ReadyTimeout(s) => StartErrorWire::ReadyTimeout { seconds: *s },
        }
    }
}

#[derive(Clone)]
pub struct ServerInstance {
    inner: Arc<Inner>,
}

struct Inner {
    work_dir: PathBuf,
    runtime: Runtime,
    state: RwLock<State>,
    event_tx: broadcast::Sender<ServerEvent>,
    log_ring: Arc<RwLock<Vec<LogLine>>>,
}

struct State {
    status: ServerStatus,
    process: Option<Arc<ServerProcess>>,
    /// 当前实例的 RCON 连接信息（启动时填，停时清）。
    rcon: Option<RconEndpoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RconEndpoint {
    pub host: String,
    pub port: u16,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct StartOptions {
    pub mc_version: String,
    pub heap_mb: u32,
    pub server_port: Option<u16>,
    pub rcon_port: Option<u16>,
    pub rcon_password: Option<String>,
    pub wait_ready_secs: u64,
    /// 显式指定 java 路径（来自 config.mc.java_path）。非空则跳过自动探测。
    pub java_path: Option<std::path::PathBuf>,
}

impl ServerInstance {
    pub fn new(work_dir: PathBuf, runtime: Runtime) -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            inner: Arc::new(Inner {
                work_dir,
                runtime,
                state: RwLock::new(State {
                    status: ServerStatus::Stopped,
                    process: None,
                    rcon: None,
                }),
                event_tx: tx,
                log_ring: Arc::new(RwLock::new(Vec::new())),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.inner.event_tx.subscribe()
    }

    pub fn runtime(&self) -> &Runtime {
        &self.inner.runtime
    }

    pub fn snapshot(&self) -> ServerSnapshot {
        let g = self.inner.state.read();
        let proc = g.process.as_ref();
        ServerSnapshot {
            status: g.status,
            pid: proc.map(|p| p.pid),
            mc_version: proc.map(|p| p.mc_version.clone()),
            started_at: proc.map(|p| p.started_at),
            work_dir: proc.map(|p| p.work_dir.clone()),
        }
    }

    pub fn tail_logs(&self, n: usize) -> Vec<LogLine> {
        let g = self.inner.log_ring.read();
        let start = g.len().saturating_sub(n);
        g[start..].to_vec()
    }

    pub fn check_environment(&self, mc_version: &str) -> anyhow::Result<EnvCheck> {
        self.inner.runtime.check_environment(mc_version)
    }

    /// 暴露当前实例的 work_dir。S3 的 plugins/files 路由用它当沙箱根。
    pub fn work_dir(&self) -> &std::path::Path {
        &self.inner.work_dir
    }

    /// 当前 RCON 连接信息（运行中才有；停了返回 None）。
    pub fn rcon_endpoint(&self) -> Option<RconEndpoint> {
        self.inner.state.read().rcon.clone()
    }

    fn set_status(&self, new: ServerStatus, pid: Option<u32>) {
        {
            let mut g = self.inner.state.write();
            g.status = new;
        }
        let _ = self.inner.event_tx.send(ServerEvent::StatusChange { status: new, pid });
    }

    pub async fn start(&self, opts: StartOptions) -> Result<ServerSnapshot, StartError> {
        // 状态校验
        {
            let g = self.inner.state.read();
            if g.status != ServerStatus::Stopped {
                return Err(StartError::BadState { status: g.status });
            }
        }

        // 环境校验
        let chk = self
            .inner
            .runtime
            .check_environment(&opts.mc_version)
            .map_err(StartError::Spawn)?;

        // Java 选取：config 显式指定 > 自动探测
        let java = if let Some(ref explicit) = opts.java_path {
            if !explicit.exists() {
                return Err(StartError::Spawn(anyhow::anyhow!(
                    "config.mc.java_path 指定的 {} 不存在",
                    explicit.display()
                )));
            }
            crate::process::inspect_java(explicit)
                .ok_or_else(|| StartError::Spawn(anyhow::anyhow!(
                    "无法识别 {} 的 Java 版本",
                    explicit.display()
                )))?
        } else {
            match chk.java {
                Some(j) => j,
                None => {
                    return Err(StartError::EnvMissing {
                        need_java_major: chk.need_java_major,
                        have_java: None,
                        paper_cached: chk.paper.is_some(),
                    });
                }
            }
        };
        let paper = match chk.paper {
            Some(p) => p,
            None => {
                return Err(StartError::EnvMissing {
                    need_java_major: chk.need_java_major,
                    have_java: Some(java.major),
                    paper_cached: false,
                });
            }
        };

        self.set_status(ServerStatus::Starting, None);

        // RCON 配置：默认 25575；密码空则自动随机一份并记录到 instance 上。
        let rcon_port = opts.rcon_port.unwrap_or(25575);
        let rcon_password = match opts.rcon_password.clone() {
            Some(p) if !p.is_empty() => p,
            _ => gen_rcon_password(),
        };

        let spawn_opts = SpawnOptions {
            java: &java.path,
            jar: &paper.jar_path,
            work_dir: &self.inner.work_dir,
            mc_version: &opts.mc_version,
            heap_mb: opts.heap_mb,
            server_port: opts.server_port,
            rcon_port: Some(rcon_port),
            rcon_password: Some(&rcon_password),
            event_tx: self.inner.event_tx.clone(),
            log_ring: self.inner.log_ring.clone(),
        };

        let (proc, mut ready_rx) = match spawn(spawn_opts).await {
            Ok(t) => t,
            Err(e) => {
                self.set_status(ServerStatus::Stopped, None);
                return Err(StartError::Spawn(e));
            }
        };
        let proc = Arc::new(proc);
        let pid = proc.pid;

        {
            let mut g = self.inner.state.write();
            g.process = Some(proc.clone());
            g.rcon = Some(RconEndpoint {
                host: "127.0.0.1".into(),
                port: rcon_port,
                password: rcon_password,
            });
        }

        // 后台 watcher：等子进程退出 → 切回 Stopped
        let this = self.clone();
        let proc_for_watch = proc.clone();
        tokio::spawn(async move {
            let _ = proc_for_watch.wait().await;
            tracing::info!("paper child exited");
            {
                let mut g = this.inner.state.write();
                g.process = None;
                g.rcon = None;
                g.status = ServerStatus::Stopped;
            }
            let _ = this
                .inner
                .event_tx
                .send(ServerEvent::StatusChange {
                    status: ServerStatus::Stopped,
                    pid: None,
                });
        });

        // 等 ready（wait_ready_secs=0 表示不限时等待）
        if opts.wait_ready_secs == 0 {
            // 无限等待模式
            match ready_rx.recv().await {
                Ok(()) => {
                    self.set_status(ServerStatus::Running, Some(pid));
                    Ok(self.snapshot())
                }
                _ => {
                    // 进程退出了但没收到 ready —— 启动失败
                    self.set_status(ServerStatus::Stopped, None);
                    Err(StartError::Spawn(anyhow::anyhow!("进程退出但未到达 Ready 状态")))
                }
            }
        } else {
            let wait = Duration::from_secs(opts.wait_ready_secs);
            match tokio::time::timeout(wait, ready_rx.recv()).await {
                Ok(Ok(())) => {
                    self.set_status(ServerStatus::Running, Some(pid));
                    Ok(self.snapshot())
                }
                _ => {
                    tracing::warn!(
                        "paper ready 超时（{}s）—— 强制终止进程并恢复状态",
                        opts.wait_ready_secs
                    );
                    proc.kill().await;
                    {
                        let mut g = self.inner.state.write();
                        g.process = None;
                        g.rcon = None;
                        g.status = ServerStatus::Stopped;
                    }
                    let _ = self.inner.event_tx.send(ServerEvent::StatusChange {
                        status: ServerStatus::Stopped,
                        pid: None,
                    });
                    Err(StartError::ReadyTimeout(opts.wait_ready_secs))
                }
            }
        }
    }

    pub async fn stop(&self, force: bool) -> anyhow::Result<()> {
        let proc = {
            let mut g = self.inner.state.write();
            if g.status == ServerStatus::Stopped {
                return Ok(());
            }
            g.status = ServerStatus::Stopping;
            g.process.clone()
        };
        let _ = self.inner.event_tx.send(ServerEvent::StatusChange {
            status: ServerStatus::Stopping,
            pid: proc.as_ref().map(|p| p.pid),
        });

        if let Some(proc) = proc {
            if !force {
                // 优雅 stop
                let _ = proc.send_stdin("stop");
                let exited = tokio::time::timeout(Duration::from_secs(15), proc.wait()).await;
                if exited.is_err() {
                    tracing::warn!("paper 15s 内未退出，强制 kill");
                    proc.kill().await;
                }
            } else {
                proc.kill().await;
            }
        }
        // watcher 会处理状态翻回 Stopped；这里不重复 set。
        Ok(())
    }

    pub async fn restart(&self, opts: StartOptions) -> Result<ServerSnapshot, StartError> {
        let _ = self.stop(false).await;
        // 等 watcher 真把 status 改回 Stopped
        for _ in 0..30 {
            if self.snapshot().status == ServerStatus::Stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.start(opts).await
    }

    pub fn send_console(&self, cmd: &str) -> anyhow::Result<()> {
        let g = self.inner.state.read();
        let proc = g
            .process
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("服务端未运行"))?;
        proc.send_stdin(cmd)
    }
}

/// 用 OsRng 生成 16 字节 → base32-lower 26 字符密码。
/// RCON 端口绑 127.0.0.1（只对本机露出），这里的 entropy 主要防同机其他进程枚举。
fn gen_rcon_password() -> String {
    use rand::RngCore;
    use rand_core::OsRng;
    let mut buf = [0u8; 16];
    OsRng.fill_bytes(&mut buf);
    // 用 hex 代替 base32 避免引入 base32 依赖到 mcserver crate
    hex::encode(&buf[..12]) // 24 字符 hex = 96 bit 熵
}

//! 顶层应用状态：单一 [`AppState`] 句柄，注入到所有 axum handler。
//!
//! 在 axum 0.7 里我们用 `Extension(state)` 而非 `with_state`（可以混着用，但
//! `Extension` 在嵌套 router 里跟 trait extractor 配合更稳）。

use std::path::PathBuf;
use std::sync::Arc;

use herald_mcserver_auth::AuthStore;
use herald_mcserver_core::Config;
use herald_mcserver_mcserver::ServerInstance;
use herald_mcserver_runtime::{Mirror, Runtime, TaskTracker};

use crate::mcp::ActivityBus;
use crate::util::rate_limit::RateLimiter;

/// 内层。永远走 `AppState = Arc<AppStateInner>`。
pub struct AppStateInner {
    pub config: Config,
    pub auth: AuthStore,
    #[allow(dead_code)]
    pub data_dir: PathBuf,
    pub setup_lock: PathBuf,
    pub tasks: TaskTracker,
    pub server: ServerInstance,
    pub mcp_activity: ActivityBus,
    pub rate_limit: Arc<RateLimiter>,
}

pub type AppState = Arc<AppStateInner>;

impl AppStateInner {
    pub fn new(config: Config, auth: AuthStore, data_dir: PathBuf) -> Self {
        let setup_lock = data_dir.join("setup.lock");
        let tasks = TaskTracker::new();

        let runtime = Runtime::new(
            data_dir.clone(),
            Mirror::new(config.runtime.mirror.clone()),
            tasks.clone(),
        );

        // server 工作目录走 paths::server_dir（v1 单实例放一个固定子目录）。
        let work_dir = herald_mcserver_core::paths::server_dir(&data_dir).join("default");
        let _ = std::fs::create_dir_all(&work_dir);
        let server = ServerInstance::new(work_dir, runtime);

        Self {
            config,
            auth,
            data_dir,
            setup_lock,
            tasks,
            server,
            mcp_activity: ActivityBus::new(),
            rate_limit: Arc::new(RateLimiter::new()),
        }
    }

    /// 已初始化 = 至少有一个 owner（DB 真相）。setup_lock 文件只是为了
    /// 在 owner 创建过程中阻挡并发提交。
    pub fn is_initialized(&self) -> bool {
        self.auth.owner_exists().unwrap_or(false)
    }
}

//! Herald-MCServerMCP 环境管家。
//!
//! 用途：在启动 Paper 之前，确保有合适版本的 Java 与 Paper jar；缺则自动下载。
//!
//! 公开模块：
//!   * [`mc_versions`] — MC → Java major 映射
//!   * [`mirrors`]     — 镜像源 URL 重写
//!   * [`java_probe`]  — 探测系统已装 Java
//!   * [`java_install`] — Adoptium Temurin 自动下载（JRE）
//!   * [`paper_dl`]    — PaperMC v2 API 客户端
//!   * [`tasks`]       — 异步任务进度跟踪 + 事件广播
//!
//! 高层入口 [`Runtime`] 把以上几块串起来：调用 [`Runtime::ensure_environment`]
//! 一次拿齐 (java_path, paper_jar)。

pub mod java_install;
pub mod java_probe;
pub mod mc_versions;
pub mod mirrors;
pub mod paper_dl;
pub mod tasks;

use std::path::PathBuf;

use anyhow::{anyhow, Result};

pub use java_probe::JavaInfo;
pub use mirrors::Mirror;
pub use paper_dl::CachedPaper;
pub use tasks::{TaskEvent, TaskHandle, TaskId, TaskKind, TaskSnapshot, TaskStatus, TaskTracker};

/// 环境就绪的结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvReady {
    pub java: JavaInfo,
    pub paper: CachedPaper,
}

/// 环境管家。所有持久化路径来自 herald-mcserver-core::paths，所以只要传 `data_dir`。
pub struct Runtime {
    pub data_dir: PathBuf,
    pub mirror: Mirror,
    pub tasks: TaskTracker,
}

impl Runtime {
    pub fn new(data_dir: impl Into<PathBuf>, mirror: Mirror, tasks: TaskTracker) -> Self {
        Self {
            data_dir: data_dir.into(),
            mirror,
            tasks,
        }
    }

    pub fn jdk_cache_root(&self) -> PathBuf {
        herald_mcserver_core::paths::jdk_cache_root(&self.data_dir)
    }

    pub fn paper_cache(&self) -> PathBuf {
        herald_mcserver_core::paths::paper_cache(&self.data_dir)
    }

    /// 列出所有候选 Java（系统 + managed 缓存）。
    pub fn probe_java(&self) -> Vec<JavaInfo> {
        let extras = self.managed_jdk_dirs();
        java_probe::probe(&extras)
    }

    /// 选择满足 `required_major` 的 Java；满足者中挑 major 最小的（最稳）。
    pub fn pick_java(&self, required_major: u32) -> Option<JavaInfo> {
        let extras = self.managed_jdk_dirs();
        java_probe::pick_for(required_major, &extras)
    }

    /// `<jdk_cache_root>/jdk-*/` —— managed JDK 缓存目录列表。
    pub fn managed_jdk_dirs(&self) -> Vec<PathBuf> {
        let root = self.jdk_cache_root();
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    out.push(e.path());
                }
            }
        }
        out
    }

    /// 触发 Java 自动安装。返回任务 ID（前端订阅 /ws 看进度）。
    pub fn install_java(&self, major: u32) -> TaskId {
        let task = self.tasks.create(TaskKind::InstallJava, format!("Java {major} (Adoptium Temurin)"));
        let id = task.id().to_string();
        let cache = self.jdk_cache_root();
        let mirror = self.mirror.clone();

        tokio::spawn(async move {
            if let Err(e) = java_install::install_temurin_jre(major, &cache, &mirror, &task).await {
                tracing::error!("install java {major} failed: {e:#}");
                task.mark_failed(format!("{e:#}"));
                return;
            }
            task.mark_done();
        });

        id
    }

    /// 触发 Paper jar 下载。
    pub fn install_paper(&self, version: String, build: Option<u64>) -> TaskId {
        let label = match build {
            Some(b) => format!("PaperMC {version} build {b}"),
            None => format!("PaperMC {version} (latest)"),
        };
        let task = self.tasks.create(TaskKind::InstallPaper, label);
        let id = task.id().to_string();
        let cache = self.paper_cache();
        let mirror = self.mirror.clone();

        tokio::spawn(async move {
            match paper_dl::ensure_paper(&version, build, &cache, &mirror, &task).await {
                Ok(_) => task.mark_done(),
                Err(e) => {
                    tracing::error!("install paper {version} failed: {e:#}");
                    task.mark_failed(format!("{e:#}"));
                }
            }
        });

        id
    }

    /// 列已缓存的 Paper jar。
    pub fn list_paper_cache(&self) -> Vec<CachedPaper> {
        paper_dl::list_cached(&self.paper_cache())
    }

    /// 准备特定 (mc_version) 启动需要的环境。同步检查；缺东西时返回 EnvMissing 让上层
    /// 决定是否启动后台下载（避免在请求线里阻塞几十秒）。
    pub fn check_environment(&self, mc_version: &str) -> Result<EnvCheck> {
        let need_major = mc_versions::required_java_major(mc_version);
        let java = self.pick_java(need_major);
        let paper = self
            .list_paper_cache()
            .into_iter()
            .filter(|p| p.version == mc_version)
            .max_by_key(|p| p.build);
        Ok(EnvCheck {
            need_java_major: need_major,
            java,
            paper,
        })
    }

    /// 同步等到环境就绪（用 install_java/install_paper 触发后轮询）。
    pub async fn wait_for_environment(
        &self,
        mc_version: &str,
        max_wait_secs: u64,
    ) -> Result<EnvReady> {
        let start = std::time::Instant::now();
        loop {
            let chk = self.check_environment(mc_version)?;
            if let (Some(java), Some(paper)) = (chk.java, chk.paper) {
                return Ok(EnvReady { java, paper });
            }
            if start.elapsed().as_secs() > max_wait_secs {
                return Err(anyhow!("等待环境就绪超时（{max_wait_secs}s）"));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvCheck {
    pub need_java_major: u32,
    pub java: Option<JavaInfo>,
    pub paper: Option<CachedPaper>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_environment_missing() {
        let dir = tempdir();
        let rt = Runtime::new(&dir, Mirror::new("default"), TaskTracker::new());
        let chk = rt.check_environment("1.21.4").unwrap();
        assert_eq!(chk.need_java_major, 21);
        // 不一定 None：跑测试的机器上系统可能装着 Java 21；只检查"need_java_major 对就行"。
        assert!(chk.paper.is_none());
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "herald-mcserver-runtime-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

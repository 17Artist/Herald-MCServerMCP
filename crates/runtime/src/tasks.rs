//! 异步下载/安装任务的进度跟踪 + 事件广播。
//!
//! 设计：
//!   * 任意一次"装 Java" / "拉 Paper" 都拿到一个 [`TaskId`]
//!   * [`TaskTracker`] 持有所有任务的快照（status + bytes downloaded/total + error）
//!   * 通过 `tokio::broadcast` 把每次进度更新广播给订阅者（HTTP /ws、CLI 等）
//!
//! 不引入数据库 —— 任务状态是进程内的；服务重启后正在跑的任务自然终止，调用方
//! 重新触发即可。完成的任务在 1 小时后自动从内存中淘汰。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;

pub type TaskId = String;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    InstallJava,
    InstallPaper,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub kind: TaskKind,
    /// 短描述，例如 "Java 21 (Adoptium Temurin)" 或 "PaperMC 1.21.4 build 230"。
    pub label: String,
    pub status: TaskStatus,
    /// 已下载字节。0 = 未知。
    pub downloaded: u64,
    /// 总字节。None = 服务端未返回 Content-Length（进度条按 spinner 处理）。
    pub total: Option<u64>,
    /// 失败时的错误信息。
    pub error: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TaskEvent {
    Snapshot { task: TaskSnapshot },
}

#[derive(Clone)]
pub struct TaskTracker {
    inner: Arc<RwLock<HashMap<TaskId, TaskSnapshot>>>,
    bus: broadcast::Sender<TaskEvent>,
}

impl Default for TaskTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskTracker {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            bus: tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.bus.subscribe()
    }

    pub fn create(&self, kind: TaskKind, label: impl Into<String>) -> TaskHandle {
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let snap = TaskSnapshot {
            id: id.clone(),
            kind,
            label: label.into(),
            status: TaskStatus::Queued,
            downloaded: 0,
            total: None,
            error: None,
            started_at: now_secs(),
            finished_at: None,
        };
        self.inner.write().insert(id.clone(), snap.clone());
        let _ = self.bus.send(TaskEvent::Snapshot { task: snap });
        TaskHandle {
            id,
            tracker: self.clone(),
        }
    }

    pub fn snapshot(&self, id: &str) -> Option<TaskSnapshot> {
        self.inner.read().get(id).cloned()
    }

    pub fn list(&self) -> Vec<TaskSnapshot> {
        self.inner.read().values().cloned().collect()
    }

    fn update<F: FnOnce(&mut TaskSnapshot)>(&self, id: &str, f: F) {
        let mut snap_clone = None;
        {
            let mut g = self.inner.write();
            if let Some(s) = g.get_mut(id) {
                f(s);
                snap_clone = Some(s.clone());
            }
        }
        if let Some(s) = snap_clone {
            let _ = self.bus.send(TaskEvent::Snapshot { task: s });
        }
    }

    /// 把"完成 + 1 小时之前"的任务从内存里清掉。调用方可以放在定时器或 GC 触发点。
    pub fn gc(&self) {
        let cutoff = now_secs() - 3600;
        let mut g = self.inner.write();
        g.retain(|_, s| {
            !(matches!(s.status, TaskStatus::Done | TaskStatus::Failed)
                && s.finished_at.map(|t| t < cutoff).unwrap_or(false))
        });
    }
}

/// 给 worker 用的 RAII 句柄。Drop 时若任务还在 Running 视为 Failed（worker 异常退出）。
pub struct TaskHandle {
    pub id: TaskId,
    tracker: TaskTracker,
}

impl TaskHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn mark_running(&self) {
        self.tracker.update(&self.id, |s| {
            s.status = TaskStatus::Running;
        });
    }

    pub fn set_total(&self, total: Option<u64>) {
        self.tracker.update(&self.id, |s| {
            s.total = total;
        });
    }

    pub fn add_progress(&self, n: u64) {
        self.tracker.update(&self.id, |s| {
            s.downloaded = s.downloaded.saturating_add(n);
        });
    }

    pub fn mark_done(self) {
        self.tracker.update(&self.id, |s| {
            s.status = TaskStatus::Done;
            s.finished_at = Some(now_secs());
        });
        std::mem::forget(self); // 避免 Drop 把 Done 改成 Failed
    }

    pub fn mark_failed(self, err: impl Into<String>) {
        let msg = err.into();
        self.tracker.update(&self.id, |s| {
            s.status = TaskStatus::Failed;
            s.error = Some(msg.clone());
            s.finished_at = Some(now_secs());
        });
        std::mem::forget(self);
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        // worker 没显式调 mark_done/mark_failed 就把它当失败处理。
        self.tracker.update(&self.id, |s| {
            if matches!(s.status, TaskStatus::Queued | TaskStatus::Running) {
                s.status = TaskStatus::Failed;
                s.error = Some("worker dropped without completion".into());
                s.finished_at = Some(now_secs());
            }
        });
    }
}

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle() {
        let t = TaskTracker::new();
        let mut rx = t.subscribe();

        let h = t.create(TaskKind::InstallJava, "Java 21");
        let id = h.id().to_string();
        h.mark_running();
        h.set_total(Some(100));
        h.add_progress(50);
        h.mark_done();

        let snap = t.snapshot(&id).unwrap();
        assert_eq!(snap.status, TaskStatus::Done);
        assert_eq!(snap.downloaded, 50);

        // 至少 4 个事件（queued/running/total/progress/done）
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count >= 4, "expected ≥4 events, got {count}");
    }

    #[test]
    fn dropped_without_completion_marks_failed() {
        let t = TaskTracker::new();
        let h = t.create(TaskKind::InstallPaper, "Paper");
        let id = h.id().to_string();
        h.mark_running();
        drop(h);
        let snap = t.snapshot(&id).unwrap();
        assert_eq!(snap.status, TaskStatus::Failed);
        assert!(snap.error.is_some());
    }
}

//! Audit log：把"敏感操作"按时间序写到 sqlite 留底。
//!
//! 简化版（v1）：只记 actor + ok + 一段 detail 文本。要查更细粒度的 IP/UA 让上层
//! 在写入前把它塞到 detail 里就行。
//!
//! 写入永远 swallow error —— audit 失败不该让正经请求失败。

use crate::{db, AuthStore};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub actor_id: Option<String>,
    pub actor_label: Option<String>,
    pub ok: bool,
    pub detail: Option<String>,
}

pub struct NewAudit<'a> {
    pub kind: &'a str,
    pub actor_id: Option<&'a str>,
    pub actor_label: Option<&'a str>,
    pub ok: bool,
    pub detail: Option<&'a str>,
}

impl AuthStore {
    pub fn audit(&self, ev: NewAudit<'_>) {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => {
                tracing::warn!("audit log: conn poisoned, skipping {}", ev.kind);
                return;
            }
        };
        let _ = conn.execute(
            "INSERT INTO audit_events (ts, kind, actor_id, actor_label, ok, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                db::now_secs(),
                ev.kind,
                ev.actor_id,
                ev.actor_label,
                if ev.ok { 1_i64 } else { 0 },
                ev.detail,
            ),
        );
    }

    /// 拉最近 N 条（默认 200，最大 1000）。倒序按时间。
    pub fn list_audit(&self, limit: usize) -> Vec<AuditEvent> {
        let limit = limit.min(1000) as i64;
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare(
            "SELECT id, ts, kind, actor_id, actor_label, ok, detail \
             FROM audit_events ORDER BY ts DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([limit], |r| {
            Ok(AuditEvent {
                id: r.get(0)?,
                ts: r.get(1)?,
                kind: r.get(2)?,
                actor_id: r.get(3)?,
                actor_label: r.get(4)?,
                ok: r.get::<_, i64>(5)? != 0,
                detail: r.get(6)?,
            })
        });
        match rows {
            Ok(it) => it.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }
}

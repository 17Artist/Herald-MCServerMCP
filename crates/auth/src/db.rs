use rusqlite::Connection;

/// 当前 schema 版本号。每次结构变更都要手动 +1 并加新分支。
pub const SCHEMA_VERSION: u32 = 2;

/// 创建表 + 把 schema_version 写到当前。幂等。
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );

        CREATE TABLE IF NOT EXISTS users (
            id              TEXT PRIMARY KEY,
            username        TEXT UNIQUE NOT NULL COLLATE NOCASE,
            password_hash   TEXT NOT NULL,
            role            TEXT NOT NULL CHECK (role IN ('owner','member')),
            created_at      INTEGER NOT NULL,
            failed_attempts INTEGER NOT NULL DEFAULT 0,
            locked_until    INTEGER
        );

        CREATE TABLE IF NOT EXISTS sessions (
            token       TEXT PRIMARY KEY,
            user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            device      TEXT,
            created_at  INTEGER NOT NULL,
            last_seen   INTEGER NOT NULL,
            expires_at  INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

        CREATE TABLE IF NOT EXISTS api_keys (
            id            TEXT PRIMARY KEY,
            owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            name          TEXT NOT NULL,
            key_hash      TEXT UNIQUE NOT NULL,
            scope         TEXT NOT NULL,
            created_at    INTEGER NOT NULL,
            last_used_at  INTEGER,
            revoked_at    INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_api_keys_owner ON api_keys(owner_user_id);
        CREATE INDEX IF NOT EXISTS idx_api_keys_hash  ON api_keys(key_hash);

        CREATE TABLE IF NOT EXISTS invites (
            code        TEXT PRIMARY KEY,
            issued_by   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            note        TEXT,
            created_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL,
            consumed_at INTEGER,
            consumed_by TEXT REFERENCES users(id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_invites_issued_by ON invites(issued_by);

        CREATE TABLE IF NOT EXISTS audit_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts          INTEGER NOT NULL,
            kind        TEXT NOT NULL,
            actor_id    TEXT,
            actor_label TEXT,
            ok          INTEGER NOT NULL,
            detail      TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_events(ts DESC);

        INSERT OR IGNORE INTO schema_version(version) VALUES (2);
    "#)?;

    // v1 → v2 升级：添加 invites / audit_events 表（CREATE IF NOT EXISTS 已经处理）。
    // 老 DB 的 schema_version 是 1，强制 update 到 2。
    let _ = conn.execute(
        "UPDATE schema_version SET version = 2 WHERE version < 2",
        [],
    );
    Ok(())
}

pub fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

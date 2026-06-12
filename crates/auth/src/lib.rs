//! Herald-MCServerMCP 认证层。
//!
//! 三种凭证：
//!   * 用户密码（owner / member）→ session token（cookie）
//!   * API Key → bearer，用于 MCP 客户端
//!
//! 公开 API：
//!   * [`AuthStore::open`] / [`AuthStore::open_in_memory`]
//!   * [`AuthStore::register_owner`] / [`register_member`] / [`login`] / [`validate_session_token`]
//!   * [`AuthStore::create_api_key`] / [`list_api_keys`] / [`revoke_api_key`] / [`validate_api_key`]
//!
//! 设计参考 herald-auth crate（已在 Herald 项目中验证）。

pub mod api_keys;
pub mod audit;
pub mod db;
pub mod error;
pub mod invites;
pub mod sessions;
pub mod users;

pub use api_keys::{ApiKey, ApiKeyScope, ApiKeyWithSecret};
pub use audit::{AuditEvent, NewAudit};
pub use error::AuthError;
pub use invites::Invite;
pub use sessions::{Session, ValidatedSession};
pub use users::{Role, User};

use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AuthStore {
    pub(crate) conn: Arc<Mutex<rusqlite::Connection>>,
}

impl AuthStore {
    pub fn open(path: &Path) -> Result<Self, AuthError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        db::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn open_in_memory() -> Result<Self, AuthError> {
        let conn = rusqlite::Connection::open_in_memory()?;
        db::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_login_session_roundtrip() {
        let store = AuthStore::open_in_memory().unwrap();
        assert!(!store.has_any_user().unwrap());

        let owner = store.register_owner("artist", "correct-horse-battery").unwrap();
        assert_eq!(owner.role, Role::Owner);
        assert!(store.has_any_user().unwrap());

        let (u, s) = store
            .login("artist", "correct-horse-battery", Some("dev-pc"), Some(60))
            .unwrap();
        assert_eq!(u.id, owner.id);
        assert!(!s.token.is_empty());

        let v = store.validate_session_token(&s.token).unwrap();
        assert_eq!(v.user.username, "artist");

        assert!(store.login("artist", "wrong", None, None).is_err());
    }

    #[test]
    fn api_key_lifecycle() {
        let store = AuthStore::open_in_memory().unwrap();
        let owner = store.register_owner("artist", "correct-horse-battery").unwrap();

        let created = store
            .create_api_key(&owner.id, "claude-desktop", ApiKeyScope::McpFull)
            .unwrap();
        assert!(created.secret.starts_with("mck_"));
        assert_eq!(created.key.scope, ApiKeyScope::McpFull);

        let (k, u) = store.validate_api_key(&created.secret).unwrap();
        assert_eq!(k.id, created.key.id);
        assert_eq!(u.id, owner.id);

        store.revoke_api_key(&owner.id, &created.key.id).unwrap();
        assert!(store.validate_api_key(&created.secret).is_err());
    }

    #[test]
    fn scope_covers_logic() {
        assert!(ApiKeyScope::McpFull.covers(ApiKeyScope::McpRead));
        assert!(ApiKeyScope::McpFull.covers(ApiKeyScope::McpFull));
        assert!(ApiKeyScope::McpRead.covers(ApiKeyScope::McpRead));
        assert!(!ApiKeyScope::McpRead.covers(ApiKeyScope::McpFull));
    }
}

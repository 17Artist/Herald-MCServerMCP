//! API Key —— 给 MCP 客户端用的 bearer token。
//!
//! 设计：
//!   * 明文：`mck_<base32 of 20 random bytes>`，前缀便于在日志/截图里一眼识别
//!   * DB 存：(id, owner_user_id, name, key_hash=sha256(key), scope, created_at, last_used_at, revoked_at)
//!   * **明文只在创建那一刻返回一次**，DB 里只存 sha256；丢了就吊销重发
//!   * 校验：`Authorization: Bearer mck_...` → sha256 → 查表 → 检查 revoked_at IS NULL
//!
//! 作用域 v1：
//!   * `mcp:full` —— 调用全部工具
//!   * `mcp:read` —— 仅只读工具（status/logs/list/probe 之类）

use crate::{db, AuthError, AuthStore};

use rand::RngCore;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiKeyScope {
    /// 全部 MCP 工具。
    #[serde(rename = "mcp:full")]
    McpFull,
    /// 仅只读 MCP 工具。
    #[serde(rename = "mcp:read")]
    McpRead,
}

impl ApiKeyScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiKeyScope::McpFull => "mcp:full",
            ApiKeyScope::McpRead => "mcp:read",
        }
    }
    pub fn parse(s: &str) -> Option<ApiKeyScope> {
        match s {
            "mcp:full" => Some(Self::McpFull),
            "mcp:read" => Some(Self::McpRead),
            _ => None,
        }
    }
    /// `full` 覆盖 `read`；`read` 不覆盖 `full`。
    pub fn covers(&self, needed: ApiKeyScope) -> bool {
        match (self, needed) {
            (ApiKeyScope::McpFull, _) => true,
            (ApiKeyScope::McpRead, ApiKeyScope::McpRead) => true,
            (ApiKeyScope::McpRead, ApiKeyScope::McpFull) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub owner_user_id: String,
    pub name: String,
    pub scope: ApiKeyScope,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyWithSecret {
    pub key: ApiKey,
    /// 明文，只在 create 时回。
    pub secret: String,
}

const KEY_PREFIX: &str = "mck_";

fn gen_key() -> String {
    let mut buf = [0u8; 20];
    OsRng.fill_bytes(&mut buf);
    let body = base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, &buf);
    format!("{KEY_PREFIX}{body}")
}

fn hash_key(plain: &str) -> String {
    let mut h = Sha256::new();
    h.update(plain.as_bytes());
    hex::encode(h.finalize())
}

impl AuthStore {
    pub fn create_api_key(
        &self,
        owner_user_id: &str,
        name: &str,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyWithSecret, AuthError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return Err(AuthError::Internal(
                "api key name must be 1-64 chars".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let secret = gen_key();
        let hash = hash_key(&secret);
        let now = db::now_secs();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO api_keys (id, owner_user_id, name, key_hash, scope, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (&id, owner_user_id, name, &hash, scope.as_str(), now),
        )?;

        Ok(ApiKeyWithSecret {
            key: ApiKey {
                id,
                owner_user_id: owner_user_id.to_string(),
                name: name.to_string(),
                scope,
                created_at: now,
                last_used_at: None,
                revoked_at: None,
            },
            secret,
        })
    }

    pub fn list_api_keys(&self, owner_user_id: &str) -> Result<Vec<ApiKey>, AuthError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, owner_user_id, name, scope, created_at, last_used_at, revoked_at \
             FROM api_keys WHERE owner_user_id=?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([owner_user_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, owner, name, scope, created, last_used, revoked) = row?;
            let scope =
                ApiKeyScope::parse(&scope).ok_or_else(|| AuthError::Internal("bad scope".into()))?;
            out.push(ApiKey {
                id,
                owner_user_id: owner,
                name,
                scope,
                created_at: created,
                last_used_at: last_used,
                revoked_at: revoked,
            });
        }
        Ok(out)
    }

    pub fn revoke_api_key(&self, owner_user_id: &str, key_id: &str) -> Result<(), AuthError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE api_keys SET revoked_at=?1 \
             WHERE id=?2 AND owner_user_id=?3 AND revoked_at IS NULL",
            (db::now_secs(), key_id, owner_user_id),
        )?;
        if n == 0 {
            return Err(AuthError::ApiKeyInvalid);
        }
        Ok(())
    }

    /// 校验 `Authorization: Bearer mck_xxx`。返回 (api_key, owner)。
    pub fn validate_api_key(
        &self,
        plain: &str,
    ) -> Result<(ApiKey, crate::users::User), AuthError> {
        if !plain.starts_with(KEY_PREFIX) {
            return Err(AuthError::ApiKeyInvalid);
        }
        let hash = hash_key(plain);

        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT k.id, k.owner_user_id, k.name, k.scope, k.created_at, k.last_used_at, k.revoked_at, \
                    u.username, u.role, u.created_at \
             FROM api_keys k JOIN users u ON u.id = k.owner_user_id \
             WHERE k.key_hash = ?1",
            [&hash],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            },
        );
        match row {
            Ok((
                id,
                owner_id,
                name,
                scope_s,
                created,
                last_used,
                revoked,
                uname,
                role,
                ucreated,
            )) => {
                if revoked.is_some() {
                    return Err(AuthError::ApiKeyInvalid);
                }
                let scope = ApiKeyScope::parse(&scope_s)
                    .ok_or_else(|| AuthError::Internal("bad scope".into()))?;
                let role = crate::users::Role::parse(&role).ok_or(AuthError::ApiKeyInvalid)?;
                let _ = conn.execute(
                    "UPDATE api_keys SET last_used_at=?1 WHERE id=?2",
                    (db::now_secs(), &id),
                );
                Ok((
                    ApiKey {
                        id,
                        owner_user_id: owner_id.clone(),
                        name,
                        scope,
                        created_at: created,
                        last_used_at: last_used,
                        revoked_at: revoked,
                    },
                    crate::users::User {
                        id: owner_id,
                        username: uname,
                        role,
                        created_at: ucreated,
                    },
                ))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(AuthError::ApiKeyInvalid),
            Err(e) => Err(e.into()),
        }
    }
}

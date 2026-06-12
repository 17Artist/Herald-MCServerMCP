//! 邀请码：Owner 给即将注册的 Member 发一次性的注册凭证。
//!
//! 设计：
//!   * 6 字符 base32-lower（短到能口报，足够 32^6 ≈ 1B 空间，配合 24h TTL 安全）
//!   * 一次性消费：注册成功后写 consumed_at + consumed_by
//!   * 默认 24h 过期；过期/已消费的 code 不能复用
//!
//! 不实现"邀请码续期"——发新的就行。

use crate::{db, AuthError, AuthStore};
use rand::RngCore;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub code: String,
    pub issued_by: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub consumed_at: Option<i64>,
    pub consumed_by: Option<String>,
}

const CODE_BYTES: usize = 5; // 5 bytes → 8 base32 chars，截到 6 char 仍然 30 bit 熵
const TTL_SECS: i64 = 24 * 3600;

fn gen_code() -> String {
    let mut buf = [0u8; CODE_BYTES];
    OsRng.fill_bytes(&mut buf);
    let s = base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, &buf);
    s.chars().take(6).collect()
}

impl AuthStore {
    /// Owner 创建邀请码。
    pub fn create_invite(
        &self,
        owner_user_id: &str,
        note: Option<&str>,
    ) -> Result<Invite, AuthError> {
        let code = gen_code();
        let now = db::now_secs();
        let expires_at = now + TTL_SECS;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO invites (code, issued_by, note, created_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (&code, owner_user_id, note, now, expires_at),
        )?;
        Ok(Invite {
            code,
            issued_by: owner_user_id.to_string(),
            note: note.map(|s| s.to_string()),
            created_at: now,
            expires_at,
            consumed_at: None,
            consumed_by: None,
        })
    }

    /// 列出 owner 创建过的邀请码（含已消费 / 已过期 / 仍有效）。
    pub fn list_invites(&self, owner_user_id: &str) -> Result<Vec<Invite>, AuthError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT code, issued_by, note, created_at, expires_at, consumed_at, consumed_by \
             FROM invites WHERE issued_by = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([owner_user_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (code, issued_by, note, created, expires, consumed, consumed_by) = row?;
            out.push(Invite {
                code,
                issued_by,
                note,
                created_at: created,
                expires_at: expires,
                consumed_at: consumed,
                consumed_by,
            });
        }
        Ok(out)
    }

    pub fn revoke_invite(&self, owner_user_id: &str, code: &str) -> Result<(), AuthError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM invites WHERE code=?1 AND issued_by=?2 AND consumed_at IS NULL",
            (code, owner_user_id),
        )?;
        if n == 0 {
            return Err(AuthError::InviteInvalid);
        }
        Ok(())
    }

    /// 用邀请码注册 Member。成功返回新建的用户 + 颁发的 session。
    pub fn redeem_invite_register(
        &self,
        code: &str,
        username: &str,
        password: &str,
        device: Option<&str>,
        ttl_secs: Option<i64>,
    ) -> Result<(crate::users::User, crate::sessions::Session), AuthError> {
        let now = db::now_secs();
        // 校验邀请码
        let issuer = {
            let conn = self.conn.lock().unwrap();
            let row: Option<(String, i64, Option<i64>)> = conn
                .query_row(
                    "SELECT issued_by, expires_at, consumed_at FROM invites WHERE code=?1",
                    [code],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok();
            let (issuer, expires_at, consumed_at) = row.ok_or(AuthError::InviteInvalid)?;
            if consumed_at.is_some() {
                return Err(AuthError::InviteConsumed);
            }
            if expires_at <= now {
                return Err(AuthError::InviteExpired);
            }
            issuer
        };

        // 创建 member
        let user = self.register_member(username, password)?;

        // 标 invite 为已消费
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE invites SET consumed_at=?1, consumed_by=?2 WHERE code=?3",
                (now, &user.id, code),
            )?;
        }

        let session = self.issue_session(&user, device, ttl_secs)?;
        let _ = issuer; // 留作 audit
        Ok((user, session))
    }

    /// Owner 删除一个 member（不能删 owner 自己）。
    pub fn delete_member(
        &self,
        owner_user_id: &str,
        target_user_id: &str,
    ) -> Result<(), AuthError> {
        if owner_user_id == target_user_id {
            return Err(AuthError::Internal("不能删除自己".into()));
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM users WHERE id=?1 AND role='member'",
            [target_user_id],
        )?;
        if n == 0 {
            return Err(AuthError::UserNotFound);
        }
        Ok(())
    }
}

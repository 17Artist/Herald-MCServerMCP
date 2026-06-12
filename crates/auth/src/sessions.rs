//! 浏览器 session：32 字节 OsRng → hex token，存 sqlite，HttpOnly cookie 走 HTTP 层。

use crate::{db, AuthError, AuthStore};
use rand::RngCore;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user_id: String,
    pub device: Option<String>,
    pub created_at: i64,
    pub last_seen: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedSession {
    pub session: Session,
    pub user: crate::users::User,
}

fn gen_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

impl AuthStore {
    pub fn issue_session(
        &self,
        user: &crate::users::User,
        device: Option<&str>,
        ttl_secs: Option<i64>,
    ) -> Result<Session, AuthError> {
        let token = gen_token();
        let now = db::now_secs();
        let expires_at = ttl_secs.map(|t| now + t);

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (token, user_id, device, created_at, last_seen, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (&token, &user.id, device, now, now, expires_at),
        )?;
        Ok(Session {
            token,
            user_id: user.id.clone(),
            device: device.map(str::to_string),
            created_at: now,
            last_seen: now,
            expires_at,
        })
    }

    pub fn validate_session_token(&self, token: &str) -> Result<ValidatedSession, AuthError> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT s.token, s.user_id, s.device, s.created_at, s.last_seen, s.expires_at,
                    u.username, u.role, u.created_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1",
            [token],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            },
        );
        match row {
            Ok((tok, uid, dev, created, seen, expires, uname, role, ucreated)) => {
                if let Some(exp) = expires {
                    if exp <= db::now_secs() {
                        conn.execute("DELETE FROM sessions WHERE token=?1", [&tok])?;
                        return Err(AuthError::SessionInvalid);
                    }
                }
                let role = crate::users::Role::parse(&role)
                    .ok_or(AuthError::SessionInvalid)?;
                conn.execute(
                    "UPDATE sessions SET last_seen=?1 WHERE token=?2",
                    (db::now_secs(), &tok),
                )?;
                Ok(ValidatedSession {
                    session: Session {
                        token: tok,
                        user_id: uid.clone(),
                        device: dev,
                        created_at: created,
                        last_seen: seen,
                        expires_at: expires,
                    },
                    user: crate::users::User {
                        id: uid,
                        username: uname,
                        role,
                        created_at: ucreated,
                    },
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(AuthError::SessionInvalid),
            Err(e) => Err(e.into()),
        }
    }

    pub fn revoke_session(&self, token: &str) -> Result<(), AuthError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE token=?1", [token])?;
        Ok(())
    }
}

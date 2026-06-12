//! 用户表（owner / member）+ 密码哈希 + 登录失败锁。
//!
//! 密码：Argon2id（OWASP 2025 起步参数：m=19MiB, t=2, p=1）。
//! 登录连续失败 10 次锁 15 分钟。
//!
//! 设计参考 herald-auth/src/users.rs（同款参数和锁策略）。

use crate::{db, AuthError, AuthStore};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Member,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Member => "member",
        }
    }
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "owner" => Some(Role::Owner),
            "member" => Some(Role::Member),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub role: Role,
    pub created_at: i64,
}

fn argon2() -> Argon2<'static> {
    let params = Params::new(19_456, 2, 1, None).expect("argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AuthError::Argon2(e.to_string()))?;
    Ok(hash.to_string())
}

fn verify_password(password: &str, hash: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(hash).map_err(|e| AuthError::Argon2(e.to_string()))?;
    Ok(argon2().verify_password(password.as_bytes(), &parsed).is_ok())
}

fn validate_username(u: &str) -> Result<(), AuthError> {
    let len = u.chars().count();
    if !(2..=32).contains(&len) {
        return Err(AuthError::InvalidUsername {
            reason: "长度需 2-32 字符",
        });
    }
    if !u
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(AuthError::InvalidUsername {
            reason: "仅允许字母/数字/._-",
        });
    }
    Ok(())
}

fn validate_password(p: &str) -> Result<(), AuthError> {
    if p.len() < 8 {
        return Err(AuthError::InvalidPassword {
            reason: "密码至少 8 个字符",
        });
    }
    if p.len() > 256 {
        return Err(AuthError::InvalidPassword {
            reason: "密码过长",
        });
    }
    Ok(())
}

impl AuthStore {
    pub fn has_any_user(&self) -> Result<bool, AuthError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n > 0)
    }

    pub fn owner_exists(&self) -> Result<bool, AuthError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role='owner'",
            [],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn register_owner(&self, username: &str, password: &str) -> Result<User, AuthError> {
        validate_username(username)?;
        validate_password(password)?;
        if self.owner_exists()? {
            return Err(AuthError::OwnerExists);
        }
        let pw = Zeroizing::new(password.to_string());
        let hash = hash_password(&pw)?;
        let id = Uuid::new_v4().to_string();
        let now = db::now_secs();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) \
             VALUES (?1, ?2, ?3, 'owner', ?4)",
            (&id, username, &hash, now),
        )?;
        Ok(User {
            id,
            username: username.to_string(),
            role: Role::Owner,
            created_at: now,
        })
    }

    pub fn register_member(&self, username: &str, password: &str) -> Result<User, AuthError> {
        validate_username(username)?;
        validate_password(password)?;
        let pw = Zeroizing::new(password.to_string());
        let hash = hash_password(&pw)?;
        let id = Uuid::new_v4().to_string();
        let now = db::now_secs();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) \
             VALUES (?1, ?2, ?3, 'member', ?4)",
            (&id, username, &hash, now),
        )?;
        Ok(User {
            id,
            username: username.to_string(),
            role: Role::Member,
            created_at: now,
        })
    }

    fn find_user_by_name(&self, username: &str) -> Result<Option<(User, String)>, AuthError> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT id, username, role, created_at, password_hash \
             FROM users WHERE username=?1 COLLATE NOCASE",
            [username],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        );
        match row {
            Ok((id, name, role, created, hash)) => {
                let role =
                    Role::parse(&role).ok_or_else(|| AuthError::Argon2("bad role".into()))?;
                Ok(Some((
                    User {
                        id,
                        username: name,
                        role,
                        created_at: created,
                    },
                    hash,
                )))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_user_by_id(&self, id: &str) -> Result<User, AuthError> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT id, username, role, created_at FROM users WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        );
        match row {
            Ok((id, name, role, created)) => {
                let role =
                    Role::parse(&role).ok_or_else(|| AuthError::Argon2("bad role".into()))?;
                Ok(User {
                    id,
                    username: name,
                    role,
                    created_at: created,
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(AuthError::UserNotFound),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_users(&self) -> Result<Vec<User>, AuthError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, role, created_at FROM users ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, role, created) = row?;
            let role =
                Role::parse(&role).ok_or_else(|| AuthError::Argon2("bad role".into()))?;
            out.push(User {
                id,
                username: name,
                role,
                created_at: created,
            });
        }
        Ok(out)
    }

    /// 登录。失败累加 failed_attempts，到 10 锁 15 分钟。
    pub fn login(
        &self,
        username: &str,
        password: &str,
        device: Option<&str>,
        ttl_secs: Option<i64>,
    ) -> Result<(User, crate::sessions::Session), AuthError> {
        let (user, hash) = self
            .find_user_by_name(username)?
            .ok_or(AuthError::UserNotFound)?;

        // lock check
        {
            let conn = self.conn.lock().unwrap();
            let locked_until: Option<i64> = conn.query_row(
                "SELECT locked_until FROM users WHERE id=?1",
                [&user.id],
                |r| r.get(0),
            )?;
            if let Some(until) = locked_until {
                if until > db::now_secs() {
                    let dt = time::OffsetDateTime::from_unix_timestamp(until)
                        .ok()
                        .and_then(|t| {
                            t.format(&time::format_description::well_known::Rfc3339).ok()
                        })
                        .unwrap_or_else(|| until.to_string());
                    return Err(AuthError::AccountLocked { until: dt });
                }
            }
        }

        let ok = verify_password(password, &hash)?;
        if !ok {
            let conn = self.conn.lock().unwrap();
            let attempts: i64 = conn.query_row(
                "SELECT failed_attempts FROM users WHERE id=?1",
                [&user.id],
                |r| r.get(0),
            )?;
            let new_attempts = attempts + 1;
            if new_attempts >= 10 {
                let until = db::now_secs() + 15 * 60;
                conn.execute(
                    "UPDATE users SET failed_attempts=?1, locked_until=?2 WHERE id=?3",
                    (new_attempts, until, &user.id),
                )?;
            } else {
                conn.execute(
                    "UPDATE users SET failed_attempts=?1 WHERE id=?2",
                    (new_attempts, &user.id),
                )?;
            }
            return Err(AuthError::WrongPassword);
        }

        // success → reset counters
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE users SET failed_attempts=0, locked_until=NULL WHERE id=?1",
                [&user.id],
            )?;
        }

        // 同 (user, device) 上的旧 session 顶掉（每设备同账号只留一条活跃）
        if let Some(dev) = device {
            let conn = self.conn.lock().unwrap();
            let _ = conn.execute(
                "DELETE FROM sessions WHERE user_id=?1 AND device=?2",
                (&user.id, dev),
            );
        }

        let session = self.issue_session(&user, device, ttl_secs)?;
        Ok((user, session))
    }
}

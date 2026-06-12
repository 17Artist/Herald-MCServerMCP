use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("argon2: {0}")]
    Argon2(String),

    #[error("username invalid: {reason}")]
    InvalidUsername { reason: &'static str },

    #[error("password invalid: {reason}")]
    InvalidPassword { reason: &'static str },

    #[error("user not found")]
    UserNotFound,

    #[error("owner already exists")]
    OwnerExists,

    #[error("wrong password")]
    WrongPassword,

    #[error("account locked until {until}")]
    AccountLocked { until: String },

    #[error("session not found or expired")]
    SessionInvalid,

    #[error("api key invalid or revoked")]
    ApiKeyInvalid,

    #[error("api key forbidden: missing scope {scope}")]
    ApiKeyScope { scope: &'static str },

    #[error("invite invalid")]
    InviteInvalid,

    #[error("invite already consumed")]
    InviteConsumed,

    #[error("invite expired")]
    InviteExpired,

    #[error("internal: {0}")]
    Internal(String),
}

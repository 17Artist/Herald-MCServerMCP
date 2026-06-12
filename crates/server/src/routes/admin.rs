//! `/api/admin/*` —— Owner 专属管理端点（用户列表 / 邀请码 / 删除 member）。
//!
//! 全部走 `OwnerUser` extractor，普通 member 直接 403。

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    middleware::auth::OwnerUser,
    state::AppState,
};

#[derive(Serialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub role: String,
    pub created_at: i64,
}

impl From<herald_mcserver_auth::User> for UserDto {
    fn from(u: herald_mcserver_auth::User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            role: u.role.as_str().to_string(),
            created_at: u.created_at,
        }
    }
}

pub async fn list_users(
    Extension(s): Extension<AppState>,
    _: OwnerUser,
) -> Result<Json<Vec<UserDto>>, ApiError> {
    let users = s.auth.list_users()?;
    Ok(Json(users.into_iter().map(UserDto::from).collect()))
}

pub async fn delete_user(
    Extension(s): Extension<AppState>,
    OwnerUser(owner): OwnerUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.auth.delete_member(&owner.id, &id)?;
    s.auth.audit(herald_mcserver_auth::NewAudit {
        kind: "user.delete",
        actor_id: Some(&owner.id),
        actor_label: Some(&owner.username),
        ok: true,
        detail: Some(&format!("deleted member id={id}")),
    });
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct InviteDto {
    pub code: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub consumed_at: Option<i64>,
    pub consumed_by: Option<String>,
}

impl From<herald_mcserver_auth::Invite> for InviteDto {
    fn from(i: herald_mcserver_auth::Invite) -> Self {
        Self {
            code: i.code,
            note: i.note,
            created_at: i.created_at,
            expires_at: i.expires_at,
            consumed_at: i.consumed_at,
            consumed_by: i.consumed_by,
        }
    }
}

pub async fn list_invites(
    Extension(s): Extension<AppState>,
    OwnerUser(owner): OwnerUser,
) -> Result<Json<Vec<InviteDto>>, ApiError> {
    let invites = s.auth.list_invites(&owner.id)?;
    Ok(Json(invites.into_iter().map(InviteDto::from).collect()))
}

#[derive(Deserialize, Default)]
pub struct CreateInviteReq {
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn create_invite(
    Extension(s): Extension<AppState>,
    OwnerUser(owner): OwnerUser,
    body: Option<Json<CreateInviteReq>>,
) -> Result<(StatusCode, Json<InviteDto>), ApiError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();
    let invite = s.auth.create_invite(&owner.id, req.note.as_deref())?;
    s.auth.audit(herald_mcserver_auth::NewAudit {
        kind: "invite.create",
        actor_id: Some(&owner.id),
        actor_label: Some(&owner.username),
        ok: true,
        detail: req.note.as_deref(),
    });
    Ok((StatusCode::CREATED, Json(InviteDto::from(invite))))
}

pub async fn revoke_invite(
    Extension(s): Extension<AppState>,
    OwnerUser(owner): OwnerUser,
    Path(code): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.auth.revoke_invite(&owner.id, &code)?;
    s.auth.audit(herald_mcserver_auth::NewAudit {
        kind: "invite.revoke",
        actor_id: Some(&owner.id),
        actor_label: Some(&owner.username),
        ok: true,
        detail: Some(&code),
    });
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub actor_id: Option<String>,
    pub actor_label: Option<String>,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Deserialize)]
pub struct AuditQ {
    #[serde(default = "default_audit_limit")]
    pub limit: usize,
}
fn default_audit_limit() -> usize { 200 }

pub async fn audit(
    Extension(s): Extension<AppState>,
    _: OwnerUser,
    axum::extract::Query(q): axum::extract::Query<AuditQ>,
) -> Json<Vec<AuditEntry>> {
    let events = s.auth.list_audit(q.limit);
    Json(events.into_iter().map(|e| AuditEntry {
        id: e.id,
        ts: e.ts,
        kind: e.kind,
        actor_id: e.actor_id,
        actor_label: e.actor_label,
        ok: e.ok,
        detail: e.detail,
    }).collect())
}

pub fn router() -> axum::Router {
    use axum::routing::{delete, get};
    axum::Router::new()
        .route("/users", get(list_users))
        .route("/users/:id", delete(delete_user))
        .route("/invites", get(list_invites).post(create_invite))
        .route("/invites/:code", delete(revoke_invite))
        .route("/audit", get(audit))
}

//! `/api/files/*` —— 服务端工作区配置文件的安全读写。
//!
//! 设计：
//!   * 只允许操作白名单内的相对路径（避免做"全文件树编辑器"，那是 S5+）
//!   * 文本类，UTF-8，单文件大小限制 1 MiB（对 server.properties / json 都够用）
//!   * 读取时如果文件不存在返回 200 + empty —— 让前端能直接打开"还没生成过"的文件做编辑
//!
//! 当前白名单（足够覆盖 S3 调试需要）：
//!   server.properties / ops.json / whitelist.json / banned-players.json / banned-ips.json
//!
//! S5 会接更宽松的"任意 plugins/<name>/config.yml"——那时再扩。

use axum::{extract::Extension, Json};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
    util::sandbox,
};

const MAX_FILE_BYTES: u64 = 1024 * 1024; // 1 MiB

const ALLOWED: &[&str] = &[
    "server.properties",
    "ops.json",
    "whitelist.json",
    "banned-players.json",
    "banned-ips.json",
    "bukkit.yml",
    "spigot.yml",
    "paper-global.yml",
    "paper-world-defaults.yml",
];

fn ensure_allowed(path: &str) -> Result<(), ApiError> {
    let normalized = path.replace('\\', "/");
    if !ALLOWED.iter().any(|p| *p == normalized) {
        return Err(ApiError::forbidden(
            "not_in_whitelist",
            format!(
                "仅允许编辑：{}",
                ALLOWED.join(" / ")
            ),
        ));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct FileEntry {
    pub path: String,
    pub exists: bool,
    pub size: u64,
}

pub async fn list(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Result<Json<Vec<FileEntry>>, ApiError> {
    let work_dir = s.server.work_dir();
    let mut out = Vec::new();
    for p in ALLOWED {
        let abs = work_dir.join(p);
        let (exists, size) = match tokio::fs::metadata(&abs).await {
            Ok(m) => (true, m.len()),
            Err(_) => (false, 0),
        };
        out.push(FileEntry {
            path: (*p).into(),
            exists,
            size,
        });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct ReadQ {
    pub path: String,
}

#[derive(Serialize)]
pub struct ReadResp {
    pub path: String,
    pub exists: bool,
    pub content: String,
    pub size: u64,
}

pub async fn read(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    axum::extract::Query(q): axum::extract::Query<ReadQ>,
) -> Result<Json<ReadResp>, ApiError> {
    ensure_allowed(&q.path)?;
    let abs = sandbox::resolve(s.server.work_dir(), &q.path)?;
    if !abs.exists() {
        return Ok(Json(ReadResp {
            path: q.path,
            exists: false,
            content: String::new(),
            size: 0,
        }));
    }
    let meta = tokio::fs::metadata(&abs).await?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(ApiError::bad_request(
            "too_large",
            format!("文件超过 {} KiB 上限", MAX_FILE_BYTES / 1024),
        ));
    }
    let content = tokio::fs::read_to_string(&abs).await.map_err(|e| {
        ApiError::bad_request("not_text", format!("文件不是 UTF-8 文本: {e}"))
    })?;
    Ok(Json(ReadResp {
        path: q.path,
        exists: true,
        content,
        size: meta.len(),
    }))
}

#[derive(Deserialize)]
pub struct WriteReq {
    pub path: String,
    pub content: String,
}

pub async fn write(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Json(req): Json<WriteReq>,
) -> Result<Json<ReadResp>, ApiError> {
    ensure_allowed(&req.path)?;
    if req.content.len() as u64 > MAX_FILE_BYTES {
        return Err(ApiError::bad_request(
            "too_large",
            format!("写入内容超过 {} KiB 上限", MAX_FILE_BYTES / 1024),
        ));
    }
    let work_dir = s.server.work_dir();
    tokio::fs::create_dir_all(work_dir).await?;
    let abs = sandbox::resolve(work_dir, &req.path)?;
    tokio::fs::write(&abs, &req.content).await?;
    let size = req.content.len() as u64;
    Ok(Json(ReadResp {
        path: req.path,
        exists: true,
        content: req.content,
        size,
    }))
}

pub fn router() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/list", get(list))
        .route("/read", get(read))
        .route("/write", post(write))
}

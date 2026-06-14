//! `/api/files/*` —— 服务端工作区文件的安全读写。
//!
//! 设计：
//!   * 路径必须在 work_dir 沙箱内（sandbox canonicalize 防越界）
//!   * 文本类，UTF-8，单文件大小限制 2 MiB
//!   * 读取时如果文件不存在返回 200 + empty —— 让前端能直接打开"还没生成过"的文件做编辑
//!   * 支持列出指定目录下的文件（默认列根目录）

use axum::{extract::Extension, Json};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
    util::sandbox,
};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

#[derive(Serialize)]
pub struct FileEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Deserialize)]
pub struct ListQ {
    /// 相对于 work_dir 的目录路径，默认 "."（根）。
    #[serde(default = "default_list_path")]
    pub path: String,
}
fn default_list_path() -> String { ".".into() }

pub async fn list(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    axum::extract::Query(q): axum::extract::Query<ListQ>,
) -> Result<Json<Vec<FileEntry>>, ApiError> {
    let work_dir = s.server.work_dir();
    let target = if q.path == "." || q.path.is_empty() {
        work_dir.to_path_buf()
    } else {
        sandbox::resolve(work_dir, &q.path)?
    };
    if !target.exists() || !target.is_dir() {
        return Ok(Json(Vec::new()));
    }
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&target).await?;
    while let Some(e) = rd.next_entry().await? {
        let meta = e.metadata().await?;
        let name = e.file_name().to_string_lossy().to_string();
        // 构造相对路径
        let rel = if q.path == "." || q.path.is_empty() {
            name
        } else {
            format!("{}/{}", q.path.trim_end_matches('/'), name)
        };
        out.push(FileEntry {
            path: rel,
            is_dir: meta.is_dir(),
            size: if meta.is_file() { meta.len() } else { 0 },
        });
    }
    out.sort_by(|a, b| {
        // 目录排前面
        b.is_dir.cmp(&a.is_dir).then(a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
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
    let work_dir = s.server.work_dir();
    let abs = sandbox::resolve(work_dir, &q.path)?;
    if !abs.exists() {
        return Ok(Json(ReadResp {
            path: q.path,
            exists: false,
            content: String::new(),
            size: 0,
        }));
    }
    let meta = tokio::fs::metadata(&abs).await?;
    if meta.is_dir() {
        return Err(ApiError::bad_request("is_directory", "目标是目录，不是文件"));
    }
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
    if req.content.len() as u64 > MAX_FILE_BYTES {
        return Err(ApiError::bad_request(
            "too_large",
            format!("写入内容超过 {} KiB 上限", MAX_FILE_BYTES / 1024),
        ));
    }
    let work_dir = s.server.work_dir();
    let abs = sandbox::resolve(work_dir, &req.path)?;
    // 自动创建父目录
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
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

//! `/api/plugins/*` —— Paper plugins/ 目录管理。
//!
//! 上传校验流程：
//!   1. 文件名校验（必须 .jar，不能含目录分隔）
//!   2. 大小限制（默认 64 MiB —— 大型插件如 Geyser 也够）
//!   3. ZIP magic 校验（前 4 字节必须是 PK\x03\x04）
//!   4. ZIP 内必须含 `plugin.yml` 或 `paper-plugin.yml`，防止上错文件
//!
//! 删除限制：
//!   * 服务端运行中也允许删（用户可能想"移除后重启"），但前端会提示用户先停服
//!   * 防止删 plugins/ 目录之外的文件 —— sandbox 卡死
//!
//! 不做"启用/禁用"开关 —— Paper 自己没有；所谓禁用就是改名为 `.jar.disabled`，
//! 留 S5 加。

use std::path::PathBuf;

use axum::{
    extract::{Extension, Multipart, Path},
    http::StatusCode,
    Json,
};
use serde::Serialize;

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
    util::sandbox::{self, validate_jar_filename},
};

const PLUGIN_MAX_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

#[derive(Serialize, Clone)]
pub struct PluginEntry {
    pub filename: String,
    pub size: u64,
    pub modified_ts: i64,
}

#[derive(Serialize)]
pub struct ListResp {
    pub plugins_dir: PathBuf,
    pub entries: Vec<PluginEntry>,
}

#[derive(Serialize, Clone)]
pub struct UploadResp {
    pub filename: String,
    pub size: u64,
    pub replaced: bool,
}

/// 公共安装逻辑：给 multipart route 和 MCP `mc_plugin_upload` 复用。
/// data = 完整的 jar 二进制 bytes。
pub fn install_plugin_bytes_sync(
    plugins_dir: &std::path::Path,
    filename: &str,
    data: &[u8],
    replace: bool,
) -> Result<UploadResp, ApiError> {
    // 大小
    if data.len() as u64 > PLUGIN_MAX_BYTES {
        return Err(ApiError::bad_request(
            "too_large",
            format!("jar 大小超过 {} MiB 上限", PLUGIN_MAX_BYTES / 1024 / 1024),
        ));
    }
    // ZIP magic
    if data.len() < 4 || &data[..4] != b"PK\x03\x04" {
        return Err(ApiError::bad_request(
            "not_zip",
            "文件不是有效的 ZIP/jar（缺 PK 头）",
        ));
    }
    // plugin.yml / paper-plugin.yml
    if !zip_contains_plugin_descriptor(data) {
        return Err(ApiError::bad_request(
            "not_paper_plugin",
            "jar 内未找到 plugin.yml / paper-plugin.yml，看起来不是 Paper 插件",
        ));
    }
    // 文件名沙箱
    let clean_name = validate_jar_filename(filename)?;
    let target = sandbox::resolve(plugins_dir, &clean_name)?;
    let exists = target.exists();
    if exists && !replace {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "already_exists",
            "目标文件已存在；如需覆盖请设 replace=true",
        ));
    }
    std::fs::create_dir_all(plugins_dir)
        .map_err(|e| ApiError::internal(format!("create plugins dir: {e}")))?;
    std::fs::write(&target, data)
        .map_err(|e| ApiError::internal(format!("write plugin: {e}")))?;
    Ok(UploadResp {
        filename: clean_name,
        size: data.len() as u64,
        replaced: exists,
    })
}

pub async fn list(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Result<Json<ListResp>, ApiError> {
    let plugins_dir = s.server.work_dir().join("plugins");
    let mut entries = Vec::new();
    if plugins_dir.exists() {
        let mut rd = tokio::fs::read_dir(&plugins_dir).await?;
        while let Some(e) = rd.next_entry().await? {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            // 只列 jar / jar.disabled（保留未来扩展）
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) {
                continue;
            }
            let meta = e.metadata().await?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            entries.push(PluginEntry {
                filename: name.to_string(),
                size: meta.len(),
                modified_ts: modified,
            });
        }
    }
    entries.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));
    Ok(Json(ListResp {
        plugins_dir,
        entries,
    }))
}

pub async fn upload(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadResp>), ApiError> {
    let mut filename: Option<String> = None;
    let mut data: Option<Vec<u8>> = None;
    let mut replace = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request("bad_multipart", e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let raw_name = field
                    .file_name()
                    .ok_or_else(|| ApiError::bad_request("missing_filename", "缺少 filename"))?
                    .to_string();
                filename = Some(validate_jar_filename(&raw_name)?);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::bad_request("read_failed", e.to_string()))?;
                if bytes.len() as u64 > PLUGIN_MAX_BYTES {
                    return Err(ApiError::bad_request(
                        "too_large",
                        format!("jar 大小超过 {} MiB 上限", PLUGIN_MAX_BYTES / 1024 / 1024),
                    ));
                }
                data = Some(bytes.to_vec());
            }
            "replace" => {
                let v = field.text().await.unwrap_or_default();
                replace = matches!(v.as_str(), "true" | "1" | "yes");
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let filename = filename.ok_or_else(|| ApiError::bad_request("missing_file", "缺少 file 字段"))?;
    let data = data.ok_or_else(|| ApiError::bad_request("missing_file", "缺少 file 字段"))?;

    let plugins_dir = s.server.work_dir().join("plugins");
    let resp = install_plugin_bytes_sync(&plugins_dir, &filename, &data, replace)?;
    Ok((StatusCode::CREATED, Json(resp)))
}

pub async fn remove(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Path(filename): Path<String>,
) -> Result<StatusCode, ApiError> {
    let plugins_dir = s.server.work_dir().join("plugins");
    let name = validate_jar_filename(&filename)
        // 也允许删 .disabled 后缀的（虽然 S3 还没启用 disable 功能）
        .or_else(|_| {
            let trimmed = filename.trim_end_matches(".disabled");
            validate_jar_filename(trimmed).map(|n| format!("{n}.disabled"))
        })?;
    let target = sandbox::resolve(&plugins_dir, &name)?;
    if !target.exists() {
        return Err(ApiError::not_found("not_found", "插件文件不存在"));
    }
    tokio::fs::remove_file(&target).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 简易扫描：用 std::io::Cursor + zip crate 翻文件名，找到 plugin.yml / paper-plugin.yml。
fn zip_contains_plugin_descriptor(data: &[u8]) -> bool {
    let cursor = std::io::Cursor::new(data);
    let Ok(mut z) = zip::ZipArchive::new(cursor) else {
        return false;
    };
    for i in 0..z.len() {
        if let Ok(file) = z.by_index(i) {
            let n = file.name();
            if n == "plugin.yml" || n == "paper-plugin.yml" {
                return true;
            }
        }
    }
    false
}

pub fn router() -> axum::Router {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route("/list", get(list))
        .route("/upload", post(upload))
        .route("/:filename", delete(remove))
}

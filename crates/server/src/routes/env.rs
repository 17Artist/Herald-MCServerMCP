//! `/api/env/*` —— 环境管家：探测、安装、查任务进度。

use axum::{
    extract::{Extension, Path},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    middleware::auth::SessionUser,
    state::AppState,
};

#[derive(Serialize)]
pub struct ProbeResp {
    pub os: &'static str,
    pub arch: &'static str,
    pub javas: Vec<herald_mcserver_runtime::JavaInfo>,
    pub managed_jdks: Vec<String>,
    pub paper_cache: Vec<herald_mcserver_runtime::CachedPaper>,
    pub default_mc_version: String,
    pub need_java_major_for_default: u32,
}

pub async fn probe(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Result<Json<ProbeResp>, ApiError> {
    let runtime = s.server.runtime();
    let javas = runtime.probe_java();
    let managed = runtime
        .managed_jdk_dirs()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    let paper_cache = runtime.list_paper_cache();
    let default_mc = s.config.mc.default_version.clone();
    let need = herald_mcserver_runtime::mc_versions::required_java_major(&default_mc);
    Ok(Json(ProbeResp {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        javas,
        managed_jdks: managed,
        paper_cache,
        default_mc_version: default_mc,
        need_java_major_for_default: need,
    }))
}

#[derive(Deserialize)]
pub struct InstallJavaReq {
    pub major: u32,
}

#[derive(Serialize)]
pub struct TaskIdResp {
    pub task_id: String,
}

pub async fn install_java(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Json(req): Json<InstallJavaReq>,
) -> Result<Json<TaskIdResp>, ApiError> {
    if !(8..=24).contains(&req.major) {
        return Err(ApiError::bad_request(
            "invalid_major",
            "Java major 必须在 [8, 24] 区间",
        ));
    }
    let id = s.server.runtime().install_java(req.major);
    Ok(Json(TaskIdResp { task_id: id }))
}

#[derive(Deserialize)]
pub struct InstallPaperReq {
    pub version: String,
    #[serde(default)]
    pub build: Option<u64>,
}

pub async fn install_paper(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Json(req): Json<InstallPaperReq>,
) -> Result<Json<TaskIdResp>, ApiError> {
    if req.version.is_empty() {
        return Err(ApiError::bad_request("invalid_version", "version 不能为空"));
    }
    let id = s
        .server
        .runtime()
        .install_paper(req.version, req.build);
    Ok(Json(TaskIdResp { task_id: id }))
}

pub async fn list_tasks(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
) -> Json<Vec<herald_mcserver_runtime::TaskSnapshot>> {
    s.tasks.gc();
    Json(s.tasks.list())
}

pub async fn task_status(
    Extension(s): Extension<AppState>,
    _user: SessionUser,
    Path(id): Path<String>,
) -> Result<Json<herald_mcserver_runtime::TaskSnapshot>, ApiError> {
    s.tasks
        .snapshot(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("task_not_found", "任务不存在或已淘汰"))
}

pub fn router() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/probe", get(probe))
        .route("/install/java", post(install_java))
        .route("/install/paper", post(install_paper))
        .route("/tasks", get(list_tasks))
        .route("/tasks/:id", get(task_status))
}

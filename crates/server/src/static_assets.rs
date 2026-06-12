//! 嵌入前端构建产物（`apps/web/dist/`）到二进制。
//!
//! 行为：
//!   * 任何匹配到嵌入资源路径的请求 → 直接返回
//!   * 否则 → 把 `index.html` 抛给 SPA 路由器（让前端自己 404）
//!
//! 编译期 `apps/web/dist` 不存在时（首次拉仓库未 npm build）—— `rust-embed`
//! 会编译失败。我们在 build.rs 里探测目录、不存在就先创建一个 stub index.html。

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/web/dist/"]
struct Asset;

/// SPA fallback：先找 path → 没有就回 index.html，让前端 router 接管。
pub async fn spa_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // 先查精确路径
    if let Some(file) = Asset::get(path) {
        return serve_embedded(path, file).into_response();
    }

    // 否则回 index.html（SPA 路由）
    match Asset::get("index.html") {
        Some(idx) => serve_embedded("index.html", idx).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "前端构建产物缺失。请在 apps/web/ 跑 `npm install && npm run build`。",
        )
            .into_response(),
    }
}

fn serve_embedded(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let body = file.data.into_owned();
    let cache = if path == "index.html" {
        "no-cache"
    } else {
        // 静态产物（JS/CSS）走 hash 命名，可以长缓存。
        "public, max-age=31536000, immutable"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache)
        .body(axum::body::Body::from(body))
        .expect("static response")
}

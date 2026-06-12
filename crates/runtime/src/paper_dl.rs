//! PaperMC 下载客户端：支持 v2（1.x 系列）和 v3（26.x 系列）双 API。
//!
//! v2 API（旧，1.7~1.21.11）：
//!   https://api.papermc.io/v2/projects/paper/versions/{ver}/builds/{b}
//!   → { downloads.application: { name, sha256 } }
//!
//! v3 API（新，26.1+）：
//!   https://fill.papermc.io/v3/projects/paper/versions/{ver}/builds
//!   → [{ id, downloads: { "server:default": { name, checksums: { sha256 }, size, url } } }]
//!
//! 版本自动路由：版本号以 "1." 开头 → v2；否则 → v3。
//!
//! 缓存：`<cache>/paper-<version>-<build>.jar`。

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::mirrors::{Mirror, Upstream};
use crate::tasks::TaskHandle;

const PAPER_API_V2: &str = "https://api.papermc.io/v2/projects/paper";
const PAPER_API_V3: &str = "https://fill.papermc.io/v3/projects/paper";

/// 判断该版本走哪个 API。
fn is_legacy_version(version: &str) -> bool {
    version.starts_with("1.")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedPaper {
    pub version: String,
    pub build: u64,
    pub jar_path: PathBuf,
    pub size: u64,
}

// ─── v3 结构（26.x+）────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct V3Build {
    id: u64,
    downloads: std::collections::HashMap<String, V3Download>,
}

#[derive(Deserialize)]
struct V3Download {
    name: String,
    checksums: V3Checksums,
    #[serde(default)]
    size: Option<u64>,
    url: String,
}

#[derive(Deserialize)]
struct V3Checksums {
    sha256: String,
}

// ─── v2 结构（1.x）──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct V2VersionInfo {
    builds: Vec<u64>,
}

#[derive(Deserialize)]
struct V2BuildInfo {
    downloads: V2Downloads,
}

#[derive(Deserialize)]
struct V2Downloads {
    application: V2Application,
}

#[derive(Deserialize)]
struct V2Application {
    name: String,
    sha256: String,
}

// ─── 公共接口 ────────────────────────────────────────────────────────────────

pub async fn latest_build(version: &str, mirror: &Mirror) -> Result<u64> {
    if is_legacy_version(version) {
        latest_build_v2(version, mirror).await
    } else {
        latest_build_v3(version, mirror).await
    }
}

/// 下载（或直接命中缓存）。返回 jar 路径。
pub async fn ensure_paper(
    version: &str,
    build: Option<u64>,
    cache_dir: &Path,
    mirror: &Mirror,
    task: &TaskHandle,
) -> Result<CachedPaper> {
    task.mark_running();
    tokio::fs::create_dir_all(cache_dir).await?;

    if is_legacy_version(version) {
        ensure_paper_v2(version, build, cache_dir, mirror, task).await
    } else {
        ensure_paper_v3(version, build, cache_dir, mirror, task).await
    }
}

/// 列出当前 cache_dir 下所有缓存到的 (version, build, path)。
pub fn list_cached(cache_dir: &Path) -> Vec<CachedPaper> {
    let mut out = Vec::new();
    let read = match std::fs::read_dir(cache_dir) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("paper-").and_then(|s| s.strip_suffix(".jar")) else {
            continue;
        };
        let mut parts = rest.rsplitn(2, '-');
        let build_str = match parts.next() {
            Some(s) => s,
            None => continue,
        };
        let version = match parts.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let build: u64 = match build_str.parse() {
            Ok(b) => b,
            Err(_) => continue,
        };
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(CachedPaper {
            version,
            build,
            jar_path: e.path(),
            size,
        });
    }
    out.sort_by(|a, b| (a.version.cmp(&b.version)).then(a.build.cmp(&b.build)));
    out
}

// ─── v3 实现（26.x+）────────────────────────────────────────────────────────

async fn latest_build_v3(version: &str, _mirror: &Mirror) -> Result<u64> {
    let url = format!("{PAPER_API_V3}/versions/{version}/builds");
    let builds: Vec<V3Build> = http_client()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    builds
        .last()
        .map(|b| b.id)
        .ok_or_else(|| anyhow!("没有 build"))
}

async fn ensure_paper_v3(
    version: &str,
    build: Option<u64>,
    cache_dir: &Path,
    _mirror: &Mirror,
    task: &TaskHandle,
) -> Result<CachedPaper> {
    // 拉 builds 列表
    let url = format!("{PAPER_API_V3}/versions/{version}/builds");
    let builds: Vec<V3Build> = http_client()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?
        .json()
        .await
        .context("decode v3 builds")?;

    let target_build = match build {
        Some(b) => builds
            .iter()
            .find(|x| x.id == b)
            .ok_or_else(|| anyhow!("build {b} not found"))?,
        None => builds
            .last()
            .ok_or_else(|| anyhow!("no builds for {version}"))?,
    };

    let dl = target_build
        .downloads
        .get("server:default")
        .ok_or_else(|| anyhow!("missing server:default download"))?;

    let cached = cache_dir.join(format!("paper-{version}-{}.jar", target_build.id));
    if cached.exists() {
        let h = sha256_file(&cached).await?;
        if h.eq_ignore_ascii_case(&dl.checksums.sha256) {
            let size = tokio::fs::metadata(&cached).await?.len();
            task.set_total(Some(size));
            task.add_progress(size);
            return Ok(CachedPaper {
                version: version.to_string(),
                build: target_build.id,
                jar_path: cached,
                size,
            });
        }
        let _ = tokio::fs::remove_file(&cached).await;
    }

    // 下载（v3 直接给了完整 URL）
    let dl_url = &dl.url;
    task.set_total(dl.size);
    tracing::info!("downloading paper {version} build {} from {dl_url}", target_build.id);

    let tmp = cache_dir.join(format!(".tmp.paper-{version}-{}.jar", target_build.id));
    let _ = tokio::fs::remove_file(&tmp).await;
    download_with_progress(dl_url, &tmp, task).await?;

    let h = sha256_file(&tmp).await?;
    if !h.eq_ignore_ascii_case(&dl.checksums.sha256) {
        let _ = tokio::fs::remove_file(&tmp).await;
        bail!("SHA-256 校验失败：want {} got {}", dl.checksums.sha256, h);
    }
    tokio::fs::rename(&tmp, &cached).await?;
    let size = tokio::fs::metadata(&cached).await?.len();
    Ok(CachedPaper {
        version: version.to_string(),
        build: target_build.id,
        jar_path: cached,
        size,
    })
}

// ─── v2 实现（1.x）──────────────────────────────────────────────────────────

async fn latest_build_v2(version: &str, mirror: &Mirror) -> Result<u64> {
    let url = mirror.rewrite(Upstream::PaperApi, &format!("{PAPER_API_V2}/versions/{version}"));
    let info: V2VersionInfo = http_client()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    info.builds
        .last()
        .copied()
        .ok_or_else(|| anyhow!("没有 build"))
}

async fn ensure_paper_v2(
    version: &str,
    build: Option<u64>,
    cache_dir: &Path,
    mirror: &Mirror,
    task: &TaskHandle,
) -> Result<CachedPaper> {
    let build = match build {
        Some(b) => b,
        None => latest_build_v2(version, mirror).await?,
    };

    let meta_url = mirror.rewrite(
        Upstream::PaperApi,
        &format!("{PAPER_API_V2}/versions/{version}/builds/{build}"),
    );
    let info: V2BuildInfo = http_client()
        .get(&meta_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .with_context(|| format!("decode build info {version} {build}"))?;

    let cached = cache_dir.join(format!("paper-{version}-{build}.jar"));
    if cached.exists() {
        let h = sha256_file(&cached).await?;
        if h.eq_ignore_ascii_case(&info.downloads.application.sha256) {
            let size = tokio::fs::metadata(&cached).await?.len();
            task.set_total(Some(size));
            task.add_progress(size);
            return Ok(CachedPaper {
                version: version.to_string(),
                build,
                jar_path: cached,
                size,
            });
        }
        let _ = tokio::fs::remove_file(&cached).await;
    }

    let dl_url = mirror.rewrite(
        Upstream::PaperApi,
        &format!(
            "{PAPER_API_V2}/versions/{version}/builds/{build}/downloads/{}",
            info.downloads.application.name
        ),
    );
    tracing::info!("downloading paper {version} build {build} from {dl_url}");

    let tmp = cache_dir.join(format!(".tmp.paper-{version}-{build}.jar"));
    let _ = tokio::fs::remove_file(&tmp).await;
    download_with_progress(&dl_url, &tmp, task).await?;

    let h = sha256_file(&tmp).await?;
    if !h.eq_ignore_ascii_case(&info.downloads.application.sha256) {
        let _ = tokio::fs::remove_file(&tmp).await;
        bail!("SHA-256 校验失败：want {} got {}", info.downloads.application.sha256, h);
    }
    tokio::fs::rename(&tmp, &cached).await?;
    let size = tokio::fs::metadata(&cached).await?.len();
    Ok(CachedPaper {
        version: version.to_string(),
        build,
        jar_path: cached,
        size,
    })
}

// ─── 工具函数 ────────────────────────────────────────────────────────────────

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Herald-MCServerMCP/0.1")
        .build()
        .expect("http client")
}

async fn download_with_progress(url: &str, out: &Path, task: &TaskHandle) -> Result<()> {
    let resp = http_client().get(url).send().await?.error_for_status()?;
    if let Some(len) = resp.content_length() {
        task.set_total(Some(len));
    }
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(out).await?;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        task.add_progress(bytes.len() as u64);
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(())
}

async fn sha256_file(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut f = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut f, &mut hasher)?;
        Ok::<_, anyhow::Error>(hex::encode(hasher.finalize()))
    })
    .await?
}

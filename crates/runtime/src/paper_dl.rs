//! PaperMC v2 API 客户端：列版本、列构建、下载 jar。
//!
//! 端点：
//!   GET /v2/projects/paper                              → { versions: [...] }
//!   GET /v2/projects/paper/versions/{ver}                → { builds: [n, ...] }
//!   GET /v2/projects/paper/versions/{ver}/builds/{b}     → { downloads.application: { name, sha256 } }
//!   GET /v2/projects/paper/versions/{ver}/builds/{b}/downloads/{name}  → jar bytes
//!
//! 缓存：`<cache>/paper-<version>-<build>.jar`。校验 SHA-256（来自上一步的 metadata）。

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::mirrors::{Mirror, Upstream};
use crate::tasks::TaskHandle;

const PAPER_API: &str = "https://api.papermc.io/v2/projects/paper";

#[derive(Deserialize)]
struct Versions {
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct VersionInfo {
    builds: Vec<u64>,
}

#[derive(Deserialize)]
struct BuildInfo {
    downloads: Downloads,
}

#[derive(Deserialize)]
struct Downloads {
    application: Application,
}

#[derive(Deserialize)]
struct Application {
    name: String,
    sha256: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedPaper {
    pub version: String,
    pub build: u64,
    pub jar_path: PathBuf,
    pub size: u64,
}

pub async fn list_versions(mirror: &Mirror) -> Result<Vec<String>> {
    let url = mirror.rewrite(Upstream::PaperApi, PAPER_API);
    let v: Versions = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "Herald-MCServerMCP/0.1")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v.versions)
}

pub async fn latest_build(version: &str, mirror: &Mirror) -> Result<u64> {
    let url = mirror.rewrite(Upstream::PaperApi, &format!("{PAPER_API}/versions/{version}"));
    let info: VersionInfo = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "Herald-MCServerMCP/0.1")
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

    let build = match build {
        Some(b) => b,
        None => latest_build(version, mirror).await?,
    };

    // 拿元数据（要 sha256 + 文件名）
    let meta_url = mirror.rewrite(
        Upstream::PaperApi,
        &format!("{PAPER_API}/versions/{version}/builds/{build}"),
    );
    let info: BuildInfo = reqwest::Client::new()
        .get(&meta_url)
        .header("User-Agent", "Herald-MCServerMCP/0.1")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .with_context(|| format!("decode build info {version} {build}"))?;

    let cached = cache_dir.join(format!("paper-{version}-{build}.jar"));
    if cached.exists() {
        // 命中缓存：仍然校验 hash，万一磁盘坏了能发现
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
            "{PAPER_API}/versions/{version}/builds/{build}/downloads/{}",
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
        bail!(
            "paper jar SHA-256 校验失败：want {} got {}",
            info.downloads.application.sha256,
            h
        );
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

async fn download_with_progress(
    url: &str,
    out: &Path,
    task: &TaskHandle,
) -> Result<()> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Herald-MCServerMCP/0.1")
        .send()
        .await?
        .error_for_status()?;
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

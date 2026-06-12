//! Adoptium Temurin JDK 自动下载 + 解压 + SHA-256 校验。
//!
//! API 端点（v3，2025+ 一直稳定）：
//!   GET https://api.adoptium.net/v3/assets/feature_releases/<major>/ga
//!       ?architecture=x64
//!       &heap_size=normal
//!       &image_type=jre
//!       &os=<linux|windows|mac>
//!       &page=0&page_size=1
//!       &project=jdk
//!       &sort_method=DEFAULT
//!       &sort_order=DESC
//!       &vendor=eclipse
//!
//! 返回里关注：
//!   binaries[0].package.{link, checksum, size}
//!   release_name              ← 用作目录命名 (jdk-21.0.5+11-jre)
//!
//! 平台选择：
//!   Windows → .zip → 用 `zip` crate 解
//!   Linux/macOS → .tar.gz → 用 `flate2` + `tar` 解
//!
//! 解压后路径：`<jdk_cache_root>/jdk-<major>/`，里面会有 release_name 作为子目录，
//! 这里我们自动把第一层目录拍平 —— 让调用方拿到的就是含 `bin/java` 的根。

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::mirrors::{Mirror, Upstream};
use crate::tasks::TaskHandle;

const ADOPTIUM_API: &str = "https://api.adoptium.net";

#[derive(Deserialize)]
struct Release {
    binaries: Vec<Binary>,
    release_name: String,
}

#[derive(Deserialize)]
struct Binary {
    package: Package,
}

#[derive(Deserialize)]
struct Package {
    link: String,
    checksum: String,
    name: String,
    #[serde(default)]
    size: Option<u64>,
}

/// 调用方传入的目标 major（17 / 21 等）+ 缓存根目录。返回安装好的 JRE 根（含 bin/java）。
pub async fn install_temurin_jre(
    major: u32,
    jdk_cache_root: &Path,
    mirror: &Mirror,
    task: &TaskHandle,
) -> Result<PathBuf> {
    task.mark_running();

    let target_dir = jdk_cache_root.join(format!("jdk-{major}"));
    if let Some(java) = existing_java(&target_dir) {
        tracing::info!("temurin {major} already installed at {}", java.display());
        return Ok(target_dir);
    }
    tokio::fs::create_dir_all(&target_dir).await?;

    let (os, archive_kind) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => ("linux", ArchiveKind::TarGz),
        ("linux", "aarch64") => ("linux", ArchiveKind::TarGz),
        ("macos", _) => ("mac", ArchiveKind::TarGz),
        ("windows", _) => ("windows", ArchiveKind::Zip),
        (o, a) => bail!("unsupported platform os={o} arch={a}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        other => bail!("unsupported arch: {other}"),
    };

    // 1) 拉 metadata
    let api_url = format!(
        "{ADOPTIUM_API}/v3/assets/feature_releases/{major}/ga?architecture={arch}\
         &heap_size=normal&image_type=jre&os={os}&page=0&page_size=1\
         &project=jdk&sort_method=DEFAULT&sort_order=DESC&vendor=eclipse"
    );
    let api_url = mirror.rewrite(Upstream::AdoptiumApi, &api_url);
    tracing::info!("temurin metadata: {api_url}");

    let client = reqwest::Client::builder()
        .user_agent("Herald-MCServerMCP/0.1")
        .build()?;

    let releases: Vec<Release> = client
        .get(&api_url)
        .send()
        .await
        .with_context(|| format!("GET {api_url}"))?
        .error_for_status()?
        .json()
        .await
        .context("decode adoptium metadata")?;

    let rel = releases
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Adoptium 没有 Java {major} 的 GA 版本"))?;

    let bin = rel
        .binaries
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Adoptium 元数据缺 binaries"))?;

    let dl_url = mirror.rewrite(Upstream::AdoptiumDownload, &bin.package.link);
    task.set_total(bin.package.size);

    // 2) 下到临时文件 + 边下边算 sha256
    let tmp = target_dir.join(format!(".tmp.{}", bin.package.name));
    let _ = tokio::fs::remove_file(&tmp).await;

    download_with_progress(&client, &dl_url, &tmp, task).await?;

    let actual_hash = sha256_file(&tmp).await?;
    if !actual_hash.eq_ignore_ascii_case(&bin.package.checksum) {
        let _ = tokio::fs::remove_file(&tmp).await;
        bail!(
            "SHA-256 校验失败：want {} got {}",
            bin.package.checksum,
            actual_hash
        );
    }

    // 3) 解压
    let extract_root = target_dir.join(".extract");
    let _ = tokio::fs::remove_dir_all(&extract_root).await;
    tokio::fs::create_dir_all(&extract_root).await?;
    extract_archive(archive_kind, &tmp, &extract_root).await?;

    // 4) 把 release_name 子目录拍平到 target_dir
    flatten_extracted(&extract_root, &target_dir, &rel.release_name).await?;

    // 清理
    let _ = tokio::fs::remove_dir_all(&extract_root).await;
    let _ = tokio::fs::remove_file(&tmp).await;

    let java = target_dir.join("bin").join(java_bin_name());
    if !java.exists() {
        bail!(
            "解压后未找到 bin/java，目录：{}",
            target_dir.display()
        );
    }
    tracing::info!("temurin {major} installed at {}", target_dir.display());
    Ok(target_dir)
}

/// 已存在 jdk-<major> 且能找到 bin/java 就返回路径。
pub fn existing_java(target_dir: &Path) -> Option<PathBuf> {
    let java = target_dir.join("bin").join(java_bin_name());
    java.exists().then_some(java)
}

fn java_bin_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

#[derive(Clone, Copy)]
enum ArchiveKind { TarGz, Zip }

async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    out: &Path,
    task: &TaskHandle,
) -> Result<()> {
    let resp = client.get(url).send().await?.error_for_status()?;
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

async fn extract_archive(kind: ArchiveKind, archive: &Path, dest: &Path) -> Result<()> {
    let archive = archive.to_path_buf();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        match kind {
            ArchiveKind::TarGz => extract_tar_gz(&archive, &dest),
            ArchiveKind::Zip => extract_zip(&archive, &dest),
        }
    })
    .await?
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(archive)?;
    let dec = flate2::read::GzDecoder::new(f);
    let mut tar = tar::Archive::new(dec);
    tar.unpack(dest)?;
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(archive)?;
    let mut z = zip::ZipArchive::new(f)?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i)?;
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut o = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut o)?;
        }
    }
    Ok(())
}

/// 把 `<extract_root>/<release_name>/...` 的内容移到 `<target>/`。
/// macOS 的 tar 格式特殊：里面是 `<release_name>/Contents/Home/...`，
/// 那种情况下我们要找含 bin/java 的层级。
async fn flatten_extracted(extract_root: &Path, target: &Path, _release_name: &str) -> Result<()> {
    let java_root = find_java_root_blocking(extract_root)
        .ok_or_else(|| anyhow!("解压结果里没有找到 bin/java"))?;

    // 把 java_root 下的所有 entry 移动到 target
    let mut entries = tokio::fs::read_dir(&java_root).await?;
    while let Some(e) = entries.next_entry().await? {
        let from = e.path();
        let to = target.join(e.file_name());
        // 已存在就先删（同 major 重装的情况）
        if to.exists() {
            if to.is_dir() {
                tokio::fs::remove_dir_all(&to).await?;
            } else {
                tokio::fs::remove_file(&to).await?;
            }
        }
        // 先尝试 rename；跨设备再 fallback 到 copy+delete
        if let Err(_) = tokio::fs::rename(&from, &to).await {
            copy_recursive(&from, &to).await?;
            if from.is_dir() {
                tokio::fs::remove_dir_all(&from).await?;
            } else {
                tokio::fs::remove_file(&from).await?;
            }
        }
    }
    Ok(())
}

fn find_java_root_blocking(start: &Path) -> Option<PathBuf> {
    let bin_name = java_bin_name();
    let mut stack = vec![start.to_path_buf()];
    while let Some(d) = stack.pop() {
        let candidate = d.join("bin").join(bin_name);
        if candidate.exists() {
            return Some(d);
        }
        if let Ok(rd) = std::fs::read_dir(&d) {
            for entry in rd.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(entry.path());
                }
            }
        }
    }
    None
}

async fn copy_recursive(from: &Path, to: &Path) -> Result<()> {
    let from = from.to_path_buf();
    let to = to.to_path_buf();
    tokio::task::spawn_blocking(move || copy_blocking(&from, &to)).await?
}

fn copy_blocking(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_blocking(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn write_at(out: &mut std::fs::File, buf: &[u8]) -> Result<()> {
    out.write_all(buf)?;
    Ok(())
}

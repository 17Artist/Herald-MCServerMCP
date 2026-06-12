//! 镜像源重写。
//!
//! 设计：用户填一个标识（"default" / "tuna" / "bmclapi" / 直接写 base url）。
//! 我们维护一张已知镜像表，把官方 URL 替换为镜像 URL。未匹配的源不重写。

/// 内部 key 区分不同 upstream，防止把 PaperMC 的 URL 替换到 Adoptium 镜像上去。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upstream {
    AdoptiumApi,        // https://api.adoptium.net
    AdoptiumDownload,   // https://github.com/adoptium/.../releases/download (从 API 返回的 link)
    PaperApi,           // https://api.papermc.io
}

impl Upstream {
    pub fn base(&self) -> &'static str {
        match self {
            Upstream::AdoptiumApi => "https://api.adoptium.net",
            Upstream::AdoptiumDownload => "https://github.com/adoptium",
            Upstream::PaperApi => "https://api.papermc.io",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mirror {
    /// 用户在 config 里填的标识。
    pub key: String,
}

impl Mirror {
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }

    /// 把官方 URL 改写为镜像 URL。任意 URL 不在我们已知映射里的就原样返回。
    pub fn rewrite(&self, upstream: Upstream, original: &str) -> String {
        match self.key.as_str() {
            "" | "default" => original.to_string(),
            "tuna" => rewrite_tuna(upstream, original),
            "bmclapi" => rewrite_bmclapi(upstream, original),
            custom if custom.starts_with("http") => {
                // 用户给了自定义 base，直接替换 upstream 的 host 段。
                rewrite_custom_base(upstream, original, custom.trim_end_matches('/'))
            }
            _ => original.to_string(),
        }
    }
}

fn rewrite_tuna(up: Upstream, url: &str) -> String {
    // 清华 TUNA 没有完整的 Adoptium 镜像，只镜像了下载文件。Paper 也没有。
    // 这里只对 Adoptium 的实际下载链接做最佳尝试，其余原样。
    match up {
        Upstream::AdoptiumDownload => url
            .replace(
                "https://github.com/adoptium/",
                "https://mirrors.tuna.tsinghua.edu.cn/Adoptium/",
            ),
        _ => url.to_string(),
    }
}

fn rewrite_bmclapi(up: Upstream, url: &str) -> String {
    match up {
        // bmclapi 不镜像 Adoptium 的元数据 API；下载文件可走 download.bmclapi.online
        // 但 path 结构不同，目前留给 Adoptium 自己（保持原 URL）。
        Upstream::AdoptiumApi | Upstream::AdoptiumDownload => url.to_string(),
        // Paper：bmclapi 提供 /paper 镜像，path 跟官方一致。
        Upstream::PaperApi => url.replace("https://api.papermc.io", "https://bmclapi2.bangbang93.com"),
    }
}

fn rewrite_custom_base(up: Upstream, url: &str, custom: &str) -> String {
    if let Some(stripped) = url.strip_prefix(up.base()) {
        format!("{custom}{stripped}")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_no_op() {
        let m = Mirror::new("default");
        let url = "https://api.papermc.io/v2/projects/paper";
        assert_eq!(m.rewrite(Upstream::PaperApi, url), url);
    }

    #[test]
    fn bmclapi_rewrites_paper() {
        let m = Mirror::new("bmclapi");
        let url = "https://api.papermc.io/v2/projects/paper/versions/1.21.4";
        let out = m.rewrite(Upstream::PaperApi, url);
        assert!(out.starts_with("https://bmclapi2.bangbang93.com/v2/"));
    }

    #[test]
    fn tuna_rewrites_adoptium_download() {
        let m = Mirror::new("tuna");
        let url = "https://github.com/adoptium/temurin21-binaries/releases/download/foo.tar.gz";
        let out = m.rewrite(Upstream::AdoptiumDownload, url);
        assert!(out.starts_with("https://mirrors.tuna.tsinghua.edu.cn/Adoptium/"));
    }

    #[test]
    fn custom_base() {
        let m = Mirror::new("https://my-cdn.example/proxy");
        let url = "https://api.papermc.io/v2/projects/paper";
        let out = m.rewrite(Upstream::PaperApi, url);
        assert_eq!(out, "https://my-cdn.example/proxy/v2/projects/paper");
    }
}

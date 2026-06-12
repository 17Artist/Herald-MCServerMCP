//! Build helper: 确保 `apps/web/dist/` 在 cargo build 时一定存在。
//!
//! 没有 SPA 构建产物时 rust-embed 会编译失败。开发期允许"先跑后端不带前端"，
//! 所以这里如果目录缺失，就生成一份占位 index.html。
//!
//! 注意：占位 HTML 会让浏览器看到一段引导文字，正式发布时务必跑 npm run build
//! 后再 cargo build。

use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../../apps/web/dist");

    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
        .join("web")
        .join("dist");

    if !dist.exists() {
        fs::create_dir_all(&dist).expect("create apps/web/dist");
    }
    let index = dist.join("index.html");
    if !index.exists() {
        let stub = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>Herald MCServerMCP</title></head>
<body style="font-family:Inter,PingFang SC,system-ui,sans-serif;background:#0a0a0c;color:#e4e4e7;padding:48px;line-height:1.6;">
<h1 style="color:#a78bfa;">Herald MCServerMCP</h1>
<p>前端尚未构建。请进入仓库根目录后执行：</p>
<pre style="background:#18181b;padding:12px;border-radius:8px;color:#c4b5fd;">cd apps/web
npm install
npm run build</pre>
<p>之后重新运行 <code style="color:#38bdf8">cargo run -p herald-mcserver</code>。</p>
</body></html>
"#;
        fs::write(&index, stub).expect("write stub index.html");
    }
}

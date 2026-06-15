//! MCP 工具表 + dispatch。
//!
//! 工具命名约定：`mc_<group>_<verb>`。group 与前端 Tab 对齐（env / server / plugin / files / rcon）。
//!
//! 每个工具的 inputSchema 严格定义参数；dispatch 在调用前不做完整 JSON Schema 校验
//! （依赖 client 自己），但每个工具内部会做"必填字段 + 类型 + 长度"基本验证。
//!
//! 输出统一封装为 MCP `content` 数组（每个 item 是 `{ type: "text", text: "..." }`），
//! 同时把结构化数据放在顶层字段方便 client 解析（spec 允许任意额外字段）。

use serde_json::{json, Value};

use crate::state::AppState;
use crate::util::sandbox;

use super::DispatchError;

#[derive(Debug, Clone, Copy)]
pub enum ToolName {
    EnvProbe,
    EnvInstallJava,
    EnvInstallPaper,
    EnvTaskStatus,

    ServerStatus,
    ServerStart,
    ServerStop,
    ServerRestart,
    ServerLogs,
    ServerExec,

    PluginList,
    PluginUploadInfo,
    PluginRemove,

    FilesList,
    FilesRead,
    FilesWrite,
    FilesReadLines,
    FilesWriteLines,
    FilesMkdir,
    FilesDelete,

    RconExec,
}

impl ToolName {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "mc_env_probe"          => Self::EnvProbe,
            "mc_env_install_java"   => Self::EnvInstallJava,
            "mc_env_install_paper"  => Self::EnvInstallPaper,
            "mc_env_task_status"    => Self::EnvTaskStatus,

            "mc_server_status"      => Self::ServerStatus,
            "mc_server_start"       => Self::ServerStart,
            "mc_server_stop"        => Self::ServerStop,
            "mc_server_restart"     => Self::ServerRestart,
            "mc_server_logs"        => Self::ServerLogs,
            "mc_server_exec"        => Self::ServerExec,

            "mc_plugin_list"        => Self::PluginList,
            "mc_plugin_upload_info" => Self::PluginUploadInfo,
            "mc_plugin_remove"      => Self::PluginRemove,

            "mc_files_list"         => Self::FilesList,
            "mc_files_read"         => Self::FilesRead,
            "mc_files_write"        => Self::FilesWrite,
            "mc_files_read_lines"   => Self::FilesReadLines,
            "mc_files_write_lines"  => Self::FilesWriteLines,
            "mc_files_mkdir"        => Self::FilesMkdir,
            "mc_files_delete"       => Self::FilesDelete,

            "mc_rcon_exec"          => Self::RconExec,

            _ => return None,
        })
    }

    /// 是否要求 `mcp:full` scope。只读工具用 `mcp:read` 也能跑。
    pub fn requires_full_scope(&self) -> bool {
        matches!(
            self,
            Self::EnvInstallJava
                | Self::EnvInstallPaper
                | Self::ServerStart
                | Self::ServerStop
                | Self::ServerRestart
                | Self::ServerExec
                | Self::PluginRemove
                | Self::FilesWrite
                | Self::FilesWriteLines
                | Self::FilesMkdir
                | Self::FilesDelete
                | Self::RconExec
        )
    }
}

/// 给 tools/list 用的静态描述。把 inputSchema 放在静态 JSON 里，避免每次 list 重新构造。
pub fn tool_list_json() -> Value {
    json!([
        {
            "name": "mc_env_probe",
            "description": "探测主机环境：OS / 架构、已检测到的 Java（path/major/vendor/source）、托管 JDK 缓存目录、Paper jar 缓存、默认 MC 版本及其所需 Java major。AI 启动调试链路时**应先调这个**。",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "mc_env_install_java",
            "description": "下载 Adoptium Temurin JRE（major 8/17/21 等）并解压到托管 JDK 缓存。返回 task_id；通过 mc_env_task_status 查进度。已安装时直接 done。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "major": { "type": "integer", "minimum": 8, "maximum": 26, "description": "Java major（如 21、25）" }
                },
                "required": ["major"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_env_install_paper",
            "description": "下载指定 MC 版本的 Paper jar。build 不填则取该版本最新构建。返回 task_id。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "version": { "type": "string", "description": "MC 版本，如 1.21.4" },
                    "build":   { "type": "integer", "description": "Paper 构建号；不填取最新" }
                },
                "required": ["version"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_env_task_status",
            "description": "查异步下载任务的进度（status/downloaded/total/error）。任务在完成后 1 小时被 GC。",
            "inputSchema": {
                "type": "object",
                "properties": { "task_id": { "type": "string" } },
                "required": ["task_id"],
                "additionalProperties": false
            }
        },

        {
            "name": "mc_server_status",
            "description": "返回 Paper 服务端实例当前状态：status (stopped/starting/running/stopping)、pid、mc_version、started_at、work_dir。",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "mc_server_start",
            "description": "启动 Paper。缺 Java/Paper 时返回结构化错误（code=env_missing），客户端可据此调 mc_env_install_*。等到 'Done!' 日志后状态切到 running 再返回。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mc_version":      { "type": "string",  "description": "不填用 config.mc.default_version" },
                    "heap_mb":         { "type": "integer", "minimum": 256 },
                    "wait_ready_secs": { "type": "integer", "minimum": 5, "default": 120 }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "mc_server_stop",
            "description": "停止 Paper：先发 stop console，15 秒超时强制 kill。force=true 直接 kill。",
            "inputSchema": {
                "type": "object",
                "properties": { "force": { "type": "boolean", "default": false } },
                "additionalProperties": false
            }
        },
        {
            "name": "mc_server_restart",
            "description": "stop + start，与 mc_server_start 接受相同参数。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mc_version":      { "type": "string" },
                    "heap_mb":         { "type": "integer", "minimum": 256 },
                    "wait_ready_secs": { "type": "integer", "minimum": 5, "default": 120 }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "mc_server_logs",
            "description": "拉最近 N 行日志（内存环最多 5000 行）。每行带 ts / stream(stdout|stderr) / text。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tail": { "type": "integer", "minimum": 1, "maximum": 5000, "default": 200 }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "mc_server_exec",
            "description": "通过子进程 stdin 发 console 命令。**不返回回包文本**（命令的输出会以日志事件流出，可在数秒后调 mc_server_logs 拿）。需要回包请改用 mc_rcon_exec。",
            "inputSchema": {
                "type": "object",
                "properties": { "command": { "type": "string", "minLength": 1 } },
                "required": ["command"],
                "additionalProperties": false
            }
        },

        {
            "name": "mc_plugin_list",
            "description": "列出 plugins/ 下所有 .jar / .jar.disabled 文件（filename/size/modified_ts）。",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "mc_plugin_upload_info",
            "description": "获取插件上传的 URL、鉴权头和用法说明。插件上传走 HTTP multipart（不走 MCP JSON-RPC），调此工具拿到完整的 curl 命令模板。",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "mc_plugin_remove",
            "description": "删除一个 plugin jar（仅文件名，无路径分隔）。删除后需要重启服务端才生效。",
            "inputSchema": {
                "type": "object",
                "properties": { "filename": { "type": "string" } },
                "required": ["filename"],
                "additionalProperties": false
            }
        },

        {
            "name": "mc_files_list",
            "description": "列出 work_dir 下指定目录的文件和子目录。默认列根目录。返回 path/is_dir/size。可用于浏览 plugins/XXX/config.yml 等任意文件。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "default": ".", "description": "相对于 work_dir 的目录路径" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "mc_files_read",
            "description": "读 work_dir 内任意文件（UTF-8 文本，≤2 MiB）。路径相对于服务端工作区，sandbox 防越界。文件不存在时返回 exists=false / content=空。",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "相对路径，如 plugins/MyPlugin/config.yml" } },
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_files_write",
            "description": "写 work_dir 内任意文件（≤2 MiB UTF-8）。父目录不存在时自动创建。路径 sandbox 防越界。修改服务端口/RCON/世界名等配置需重启 Paper 才生效。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path":    { "type": "string", "description": "相对路径" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_files_read_lines",
            "description": "读文件的指定行范围。适合大文件只看一段（如日志、长配置）。行号从 1 开始。不指定 end 则读到文件末尾。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path":  { "type": "string", "description": "相对路径" },
                    "start": { "type": "integer", "minimum": 1, "description": "起始行号（含）" },
                    "end":   { "type": "integer", "minimum": 1, "description": "结束行号（含），不填则到末尾" }
                },
                "required": ["path", "start"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_files_write_lines",
            "description": "替换文件中指定行范围的内容。行号从 1 开始。start~end 范围被 new_content 替换（可以是不同行数，实现插入/删除/替换）。文件不存在时创建。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path":        { "type": "string", "description": "相对路径" },
                    "start":       { "type": "integer", "minimum": 1, "description": "起始行号（含）" },
                    "end":         { "type": "integer", "minimum": 1, "description": "结束行号（含）" },
                    "new_content": { "type": "string", "description": "替换进去的文本（可含多行）" }
                },
                "required": ["path", "start", "end", "new_content"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_files_mkdir",
            "description": "创建目录（含中间目录）。用于在上传插件前预建 plugins/XXX/。",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "相对目录路径" } },
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "mc_files_delete",
            "description": "删除一个文件。不支持删目录（防误删世界）。",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "相对文件路径" } },
                "required": ["path"],
                "additionalProperties": false
            }
        },

        {
            "name": "mc_rcon_exec",
            "description": "通过 RCON 发命令并**返回回包文本**（与 mc_server_exec 的区别）。仅运行中可用；端点是 127.0.0.1，密码每次启动随机生成。",
            "inputSchema": {
                "type": "object",
                "properties": { "command": { "type": "string", "minLength": 1 } },
                "required": ["command"],
                "additionalProperties": false
            }
        }
    ])
}

pub async fn call_tool(
    state: AppState,
    name: ToolName,
    args: Value,
) -> Result<Value, DispatchError> {
    match name {
        ToolName::EnvProbe => env_probe(&state).await,
        ToolName::EnvInstallJava => env_install_java(&state, &args).await,
        ToolName::EnvInstallPaper => env_install_paper(&state, &args).await,
        ToolName::EnvTaskStatus => env_task_status(&state, &args).await,

        ToolName::ServerStatus => server_status(&state).await,
        ToolName::ServerStart => server_start_or_restart(&state, &args, false).await,
        ToolName::ServerRestart => server_start_or_restart(&state, &args, true).await,
        ToolName::ServerStop => server_stop(&state, &args).await,
        ToolName::ServerLogs => server_logs(&state, &args).await,
        ToolName::ServerExec => server_exec(&state, &args).await,

        ToolName::PluginList => plugin_list(&state).await,
        ToolName::PluginUploadInfo => plugin_upload_info(&state).await,
        ToolName::PluginRemove => plugin_remove(&state, &args).await,

        ToolName::FilesList => files_list(&state, &args).await,
        ToolName::FilesRead => files_read(&state, &args).await,
        ToolName::FilesWrite => files_write(&state, &args).await,
        ToolName::FilesReadLines => files_read_lines(&state, &args).await,
        ToolName::FilesWriteLines => files_write_lines(&state, &args).await,
        ToolName::FilesMkdir => files_mkdir(&state, &args).await,
        ToolName::FilesDelete => files_delete(&state, &args).await,

        ToolName::RconExec => rcon_exec(&state, &args).await,
    }
}

// ---- helpers ----------------------------------------------------------------

fn text_content(text: impl Into<String>) -> Value {
    json!([{ "type": "text", "text": text.into() }])
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, DispatchError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| DispatchError::invalid_params(format!("缺少字段或类型不对：{key}")))
}

fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| v.as_u64())
}

// ---- env --------------------------------------------------------------------

async fn env_probe(state: &AppState) -> Result<Value, DispatchError> {
    let runtime = state.server.runtime();
    let javas = runtime.probe_java();
    let managed: Vec<String> = runtime
        .managed_jdk_dirs()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    let paper_cache = runtime.list_paper_cache();
    let default_mc = state.config.mc.default_version.clone();
    let need = herald_mcserver_runtime::mc_versions::required_java_major(&default_mc);

    // 连接信息（AI 客户端需要知道玩家/测试客户端该连哪里）
    let server_port = state.config.mc.server_port;
    let public_host = if state.config.server.public_host.is_empty() {
        // fallback：从 public_url 提取 host；再 fallback 到 listen IP
        extract_host(&state.config.server.public_url)
            .unwrap_or_else(|| {
                state.config.server.listen
                    .split(':')
                    .next()
                    .unwrap_or("127.0.0.1")
                    .to_string()
            })
    } else {
        state.config.server.public_host.clone()
    };

    let summary = format!(
        "OS={} arch={}; default MC={} (needs Java {}+); javas={}; paper jars cached={}; mc_address={}:{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        default_mc,
        need,
        javas.len(),
        paper_cache.len(),
        public_host,
        server_port,
    );

    Ok(json!({
        "content": text_content(summary),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "javas": javas,
        "managed_jdks": managed,
        "paper_cache": paper_cache,
        "default_mc_version": default_mc,
        "need_java_major_for_default": need,
        "server_host": public_host,
        "server_port": server_port,
    }))
}

/// 从 URL 里提取 host 部分（不含 scheme/port/path）。
fn extract_host(url: &str) -> Option<String> {
    let stripped = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = stripped.split('/').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() { None } else { Some(host.to_string()) }
}

async fn env_install_java(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let major = args
        .get("major")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| DispatchError::invalid_params("major 必填且为整数"))?;
    if !(8..=26).contains(&major) {
        return Err(DispatchError::invalid_params(
            "major 取值必须在 [8, 26]",
        ));
    }
    let id = state.server.runtime().install_java(major as u32);
    Ok(json!({
        "content": text_content(format!("已排队下载 Java {major}（task_id={id}）")),
        "task_id": id,
    }))
}

async fn env_install_paper(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let version = require_str(args, "version")?.to_string();
    let build = opt_u64(args, "build");
    let id = state.server.runtime().install_paper(version.clone(), build);
    Ok(json!({
        "content": text_content(format!(
            "已排队下载 Paper {version}（task_id={id}）"
        )),
        "task_id": id,
    }))
}

async fn env_task_status(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let task_id = require_str(args, "task_id")?;
    let snap = state
        .tasks
        .snapshot(task_id)
        .ok_or_else(|| DispatchError::tool(format!("任务不存在或已淘汰: {task_id}")))?;
    let summary = format!(
        "{} · {} · {}",
        snap.label,
        match snap.status {
            herald_mcserver_runtime::TaskStatus::Queued => "queued",
            herald_mcserver_runtime::TaskStatus::Running => "running",
            herald_mcserver_runtime::TaskStatus::Done => "done",
            herald_mcserver_runtime::TaskStatus::Failed => "failed",
        },
        match snap.total {
            Some(t) if t > 0 => format!(
                "{:.1}/{:.1} MB",
                snap.downloaded as f64 / 1024.0 / 1024.0,
                t as f64 / 1024.0 / 1024.0
            ),
            _ => format!("{:.1} MB", snap.downloaded as f64 / 1024.0 / 1024.0),
        }
    );
    Ok(json!({
        "content": text_content(summary),
        "task": snap,
    }))
}

// ---- server -----------------------------------------------------------------

async fn server_status(state: &AppState) -> Result<Value, DispatchError> {
    let snap = state.server.snapshot();
    let summary = format!(
        "status={:?} pid={} mc_version={}",
        snap.status,
        snap.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
        snap.mc_version.clone().unwrap_or_else(|| "-".into())
    );
    Ok(json!({
        "content": text_content(summary),
        "snapshot": snap,
    }))
}

async fn server_start_or_restart(
    state: &AppState,
    args: &Value,
    restart: bool,
) -> Result<Value, DispatchError> {
    let mc_version = args
        .get("mc_version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| state.config.mc.default_version.clone());
    let heap_mb = opt_u64(args, "heap_mb").unwrap_or(state.config.mc.heap_mb as u64) as u32;
    let wait_ready_secs = opt_u64(args, "wait_ready_secs").unwrap_or(0);

    let opts = herald_mcserver_mcserver::StartOptions {
        mc_version,
        heap_mb,
        server_port: None,
        rcon_port: Some(state.config.mc.rcon_port),
        rcon_password: {
            let pw = state.config.mc.rcon_password.clone();
            if pw.is_empty() { None } else { Some(pw) }
        },
        wait_ready_secs,
        java_path: {
            let p = state.config.mc.java_path.trim();
            if p.is_empty() { None } else { Some(std::path::PathBuf::from(p)) }
        },
    };

    let result = if restart {
        state.server.restart(opts).await
    } else {
        state.server.start(opts).await
    };

    match result {
        Ok(snap) => Ok(json!({
            "content": text_content(format!(
                "服务端已启动 pid={} mc_version={} address={}:{}",
                snap.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                snap.mc_version.clone().unwrap_or_else(|| "-".into()),
                state.config.server.public_host,
                state.config.mc.server_port,
            )),
            "snapshot": snap,
            "server_host": state.config.server.public_host,
            "server_port": state.config.mc.server_port,
        })),
        Err(e) => {
            // 把结构化错误吐到 content text 里，AI 可读；也透出 wire 形式
            let wire = herald_mcserver_mcserver::StartErrorWire::from(&e);
            let body_text = format!("启动失败: {e}");
            Err(DispatchError {
                code: super::codes::TOOL_ERROR,
                message: serde_json::to_string(&json!({
                    "summary": body_text,
                    "structured": wire,
                }))
                .unwrap_or(body_text),
            })
        }
    }
}

async fn server_stop(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    state
        .server
        .stop(force)
        .await
        .map_err(|e| DispatchError::tool(e.to_string()))?;
    // 等到状态翻回 stopped，最多 5s
    for _ in 0..50 {
        if state.server.snapshot().status == herald_mcserver_mcserver::ServerStatus::Stopped {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let snap = state.server.snapshot();
    Ok(json!({
        "content": text_content("服务端已停止"),
        "snapshot": snap,
    }))
}

async fn server_logs(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let tail = opt_u64(args, "tail").unwrap_or(200) as usize;
    let lines = state.server.tail_logs(tail.min(5000));
    let text: String = lines
        .iter()
        .map(|l| {
            if l.stream == "stderr" {
                format!("[err] {}", l.text)
            } else {
                l.text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(json!({
        "content": text_content(if text.is_empty() { "（暂无日志）".into() } else { text }),
        "lines": lines,
    }))
}

async fn server_exec(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let cmd = require_str(args, "command")?.trim();
    if cmd.is_empty() {
        return Err(DispatchError::invalid_params("command 不能为空"));
    }
    state
        .server
        .send_console(cmd)
        .map_err(|e| DispatchError::tool(e.to_string()))?;
    Ok(json!({
        "content": text_content(format!(
            "已通过 stdin 发送命令: {cmd}\n回包不会随这条响应返回；几秒后调 mc_server_logs 看输出。"
        )),
    }))
}

// ---- plugin -----------------------------------------------------------------

async fn plugin_list(state: &AppState) -> Result<Value, DispatchError> {
    let plugins_dir = state.server.work_dir().join("plugins");
    let mut entries: Vec<Value> = Vec::new();
    if plugins_dir.exists() {
        let mut rd = tokio::fs::read_dir(&plugins_dir)
            .await
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        while let Some(e) = rd
            .next_entry()
            .await
            .map_err(|er| DispatchError::internal(er.to_string()))?
        {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) {
                continue;
            }
            let meta = e
                .metadata()
                .await
                .map_err(|er| DispatchError::internal(er.to_string()))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            entries.push(json!({
                "filename": name,
                "size": meta.len(),
                "modified_ts": modified,
            }));
        }
    }
    let summary = format!(
        "plugins_dir={} · 共 {} 个 jar",
        plugins_dir.display(),
        entries.len()
    );
    Ok(json!({
        "content": text_content(summary),
        "plugins_dir": plugins_dir.display().to_string(),
        "entries": entries,
    }))
}

async fn plugin_remove(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let filename = require_str(args, "filename")?;
    let plugins_dir = state.server.work_dir().join("plugins");

    // 复用 sandbox 的文件名校验（也支持 .disabled）
    let name = sandbox::validate_jar_filename(filename)
        .or_else(|_| {
            let trimmed = filename.trim_end_matches(".disabled");
            sandbox::validate_jar_filename(trimmed).map(|n| format!("{n}.disabled"))
        })
        .map_err(|e| DispatchError::invalid_params(format!("文件名不合法: {e}")))?;
    let target = sandbox::resolve(&plugins_dir, &name)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;

    if !target.exists() {
        return Err(DispatchError::tool(format!(
            "插件不存在: {filename}"
        )));
    }
    tokio::fs::remove_file(&target)
        .await
        .map_err(|e| DispatchError::tool(e.to_string()))?;
    Ok(json!({
        "content": text_content(format!("已删除 {name}")),
    }))
}

async fn plugin_upload_info(state: &AppState) -> Result<Value, DispatchError> {
    let base = if !state.config.server.public_url.is_empty() {
        state.config.server.public_url.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", state.config.server.listen)
    };
    let upload_url = format!("{base}/api/plugins/upload");

    let usage = format!(
        "插件上传走 HTTP multipart（不走 MCP JSON-RPC）。\n\
         \n\
         URL:    POST {upload_url}\n\
         Header: Authorization: Bearer <你当前使用的同一把 API Key>\n\
         Body:   multipart/form-data\n\
         Fields:\n\
         - file: .jar 文件（必填）\n\
         - replace: true/false（同名是否覆盖，默认 false）\n\
         \n\
         curl 示例：\n\
         curl -X POST \\\n\
           -H \"Authorization: Bearer <your_key>\" \\\n\
           -F \"file=@MyPlugin.jar\" \\\n\
           -F \"replace=true\" \\\n\
           {upload_url}\n\
         \n\
         校验：ZIP magic + 必须含 plugin.yml 或 paper-plugin.yml。\n\
         上传后需 mc_server_restart 才加载。"
    );

    Ok(json!({
        "content": text_content(usage),
        "upload_url": upload_url,
        "method": "POST",
        "content_type": "multipart/form-data",
        "auth": "Bearer <同一把 API Key>",
        "fields": {
            "file": ".jar 文件（必填）",
            "replace": "true/false（可选，默认 false）"
        },
    }))
}

// ---- files ------------------------------------------------------------------

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

async fn files_list(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let work_dir = state.server.work_dir();
    let dir_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let target = if dir_path == "." || dir_path.is_empty() {
        work_dir.to_path_buf()
    } else {
        sandbox::resolve(work_dir, dir_path)
            .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?
    };
    if !target.exists() || !target.is_dir() {
        return Ok(json!({
            "content": text_content(format!("目录不存在: {dir_path}")),
            "entries": [],
        }));
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut rd = tokio::fs::read_dir(&target)
        .await
        .map_err(|e| DispatchError::internal(e.to_string()))?;
    while let Some(e) = rd
        .next_entry()
        .await
        .map_err(|e| DispatchError::internal(e.to_string()))?
    {
        let meta = e.metadata().await.map_err(|e| DispatchError::internal(e.to_string()))?;
        let name = e.file_name().to_string_lossy().to_string();
        let rel = if dir_path == "." || dir_path.is_empty() {
            name
        } else {
            format!("{}/{}", dir_path.trim_end_matches('/'), name)
        };
        entries.push(json!({
            "path": rel,
            "is_dir": meta.is_dir(),
            "size": if meta.is_file() { meta.len() } else { 0 },
        }));
    }
    let summary = format!("{} 下共 {} 项", dir_path, entries.len());
    Ok(json!({
        "content": text_content(summary),
        "entries": entries,
    }))
}

async fn files_read(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let path = require_str(args, "path")?;
    let abs = sandbox::resolve(state.server.work_dir(), path)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;
    if !abs.exists() {
        return Ok(json!({
            "content": text_content(format!("{path} 尚未生成")),
            "path": path,
            "exists": false,
            "content_text": "",
        }));
    }
    let meta = tokio::fs::metadata(&abs)
        .await
        .map_err(|e| DispatchError::internal(e.to_string()))?;
    if meta.is_dir() {
        return Err(DispatchError::invalid_params("目标是目录，请用 mc_files_list"));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(DispatchError::tool(format!(
            "文件超过 {} KiB 上限",
            MAX_FILE_BYTES / 1024
        )));
    }
    let text = tokio::fs::read_to_string(&abs)
        .await
        .map_err(|e| DispatchError::tool(format!("文件不是 UTF-8 文本: {e}")))?;
    Ok(json!({
        "content": text_content(text.clone()),
        "path": path,
        "exists": true,
        "content_text": text,
        "size": meta.len(),
    }))
}

async fn files_write(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let path = require_str(args, "path")?;
    let content = require_str(args, "content")?;
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(DispatchError::invalid_params(format!(
            "写入内容超过 {} KiB 上限",
            MAX_FILE_BYTES / 1024
        )));
    }
    let work_dir = state.server.work_dir();
    let abs = sandbox::resolve(work_dir, path)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;
    // 自动创建父目录
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| DispatchError::internal(e.to_string()))?;
    }
    tokio::fs::write(&abs, content)
        .await
        .map_err(|e| DispatchError::tool(e.to_string()))?;
    Ok(json!({
        "content": text_content(format!("已写入 {path}（{} 字节）", content.len())),
        "path": path,
        "size": content.len(),
    }))
}

async fn files_read_lines(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let path = require_str(args, "path")?;
    let start = args.get("start").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let end = args.get("end").and_then(|v| v.as_u64()).map(|v| v as usize);

    let abs = sandbox::resolve(state.server.work_dir(), path)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;
    if !abs.exists() {
        return Err(DispatchError::tool(format!("文件不存在: {path}")));
    }
    let meta = tokio::fs::metadata(&abs).await.map_err(|e| DispatchError::internal(e.to_string()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(DispatchError::tool(format!("文件超过 {} KiB 上限", MAX_FILE_BYTES / 1024)));
    }
    let full = tokio::fs::read_to_string(&abs)
        .await
        .map_err(|e| DispatchError::tool(format!("不是 UTF-8: {e}")))?;
    let all_lines: Vec<&str> = full.lines().collect();
    let total = all_lines.len();
    let s = start.max(1) - 1; // 转 0-based
    let e = end.unwrap_or(total).min(total);
    if s >= total {
        return Ok(json!({
            "content": text_content(format!("文件共 {total} 行，start={start} 超出范围")),
            "path": path,
            "total_lines": total,
            "lines": "",
        }));
    }
    let slice = &all_lines[s..e];
    let text = slice.join("\n");
    Ok(json!({
        "content": text_content(text.clone()),
        "path": path,
        "start": s + 1,
        "end": e,
        "total_lines": total,
        "lines": text,
    }))
}

async fn files_write_lines(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let path = require_str(args, "path")?;
    let start = args.get("start").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let end = args.get("end").and_then(|v| v.as_u64()).unwrap_or(start as u64) as usize;
    let new_content = require_str(args, "new_content")?;

    let work_dir = state.server.work_dir();
    let abs = sandbox::resolve(work_dir, path)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;

    // 读原文件（不存在则当空文件）
    let original = if abs.exists() {
        tokio::fs::read_to_string(&abs)
            .await
            .map_err(|e| DispatchError::tool(format!("不是 UTF-8: {e}")))?
    } else {
        String::new()
    };

    let mut lines: Vec<&str> = original.lines().collect();
    let total = lines.len();

    // 转 0-based，clamp
    let s = (start.max(1) - 1).min(total);
    let e = end.min(total);
    if s > e {
        return Err(DispatchError::invalid_params("start 不能大于 end"));
    }

    // 替换 [s..e] 为 new_content 的行
    let new_lines: Vec<&str> = new_content.lines().collect();
    lines.splice(s..e, new_lines.iter().copied());

    let result = lines.join("\n");
    if result.len() as u64 > MAX_FILE_BYTES {
        return Err(DispatchError::tool(format!("结果超过 {} KiB 上限", MAX_FILE_BYTES / 1024)));
    }
    // 自动创建父目录
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| DispatchError::internal(e.to_string()))?;
    }
    tokio::fs::write(&abs, &result).await.map_err(|e| DispatchError::tool(e.to_string()))?;

    let new_total = result.lines().count();
    Ok(json!({
        "content": text_content(format!(
            "已替换 {path} 第 {start}~{end} 行 → {} 行新内容（共 {new_total} 行）",
            new_content.lines().count()
        )),
        "path": path,
        "total_lines": new_total,
    }))
}

async fn files_mkdir(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let path = require_str(args, "path")?;
    let abs = sandbox::resolve(state.server.work_dir(), path)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;
    tokio::fs::create_dir_all(&abs).await.map_err(|e| DispatchError::tool(e.to_string()))?;
    Ok(json!({
        "content": text_content(format!("已创建目录 {path}")),
        "path": path,
    }))
}

async fn files_delete(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let path = require_str(args, "path")?;
    let abs = sandbox::resolve(state.server.work_dir(), path)
        .map_err(|e| DispatchError::invalid_params(format!("路径不合法: {e}")))?;
    if !abs.exists() {
        return Err(DispatchError::tool(format!("文件不存在: {path}")));
    }
    if abs.is_dir() {
        return Err(DispatchError::tool("不支持删目录（防误删世界），请用具体文件路径"));
    }
    tokio::fs::remove_file(&abs).await.map_err(|e| DispatchError::tool(e.to_string()))?;
    Ok(json!({
        "content": text_content(format!("已删除 {path}")),
        "path": path,
    }))
}

// ---- rcon -------------------------------------------------------------------

async fn rcon_exec(state: &AppState, args: &Value) -> Result<Value, DispatchError> {
    let cmd = require_str(args, "command")?.trim().to_string();
    if cmd.is_empty() {
        return Err(DispatchError::invalid_params("command 不能为空"));
    }
    if cmd.len() > herald_mcserver_rcon::MAX_PAYLOAD {
        return Err(DispatchError::invalid_params(format!(
            "命令超过 {} 字节上限",
            herald_mcserver_rcon::MAX_PAYLOAD
        )));
    }

    let endpoint = state
        .server
        .rcon_endpoint()
        .ok_or_else(|| DispatchError::tool("服务端未运行（RCON 端点未就绪）"))?;
    let addr = format!("{}:{}", endpoint.host, endpoint.port);
    let mut client = herald_mcserver_rcon::RconClient::connect(&addr, &endpoint.password)
        .await
        .map_err(|e| DispatchError::tool(format!("RCON 连接失败: {e}")))?;
    let response = client
        .exec(&cmd)
        .await
        .map_err(|e| DispatchError::tool(format!("RCON 执行失败: {e}")))?;
    Ok(json!({
        "content": text_content(response.clone()),
        "command": cmd,
        "response": response,
    }))
}

<p align="center">
  <img src="apps/web/src/assets/logo.svg" width="72" height="72" alt="Herald MCServerMCP" />
</p>

<h1 align="center">Herald MCServerMCP</h1>

<p align="center">
  Minecraft 插件开发AI远程调试工具<br/>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.75+-orange?logo=rust" alt="Rust 1.75+" />
  <img src="https://img.shields.io/badge/paper-1.16~1.21.x-green?logo=minecraft" alt="Paper" />
  <img src="https://img.shields.io/badge/MCP-2024--11--05-blue" alt="MCP Protocol" />
  <img src="https://img.shields.io/badge/license-GPL--3.0-purple" alt="License" />
</p>


---

## 用法

```powershell
# 编辑 config.toml 填你的 IP 和端口
.\herald-mcserver.exe --config config.toml
```

- 打开 `http://localhost:8787`：创建管理员 → MCP Keys → 把 JSON 粘到 AI 工具里。
- 请注意，该工具是为了便于AI工作流调试，请不要接入任何生产环境。它是个测试工具不是一个正式环境的组件。
- 此外，安全相关不做任何保证和相关维护（由于本工具仅用于AI调试，大部分代码是Vibe Coding，并没有经过过多人工审查）。
---

## 配置

```toml
[server]
listen      = "0.0.0.0:8787"    # Web/MCP 端口
public_host = ""                 # 你的外网 IP 或域名

[mc]
default_version = "1.21.4"
heap_mb         = 4096
server_port     = 25565          # MC 游戏端口
```

只需开放两个端口：**8787**（MCP + 面板）和 **25565**（MC 客户端连入，按需）。

完整配置见 `config.example.toml`。

---

## MCP 工具（17 个）

| 组   | 工具                                                                                                                    |
|-----|-----------------------------------------------------------------------------------------------------------------------|
| 环境  | `mc_env_probe` · `mc_env_install_java` · `mc_env_install_paper` · `mc_env_task_status`                                |
| 服务端 | `mc_server_status` · `mc_server_start` · `mc_server_stop` · `mc_server_restart` · `mc_server_logs` · `mc_server_exec` |
| 插件  | `mc_plugin_list` · `mc_plugin_upload` · `mc_plugin_remove`                                                            |
| 文件  | `mc_files_list` · `mc_files_read` · `mc_files_write`                                                                  |
| 命令  | `mc_rcon_exec`                                                                                                        |


---

## 从源码构建

```bash
cd apps/web && npm install && npm run build && cd ../..
cargo build --release -p herald-mcserver
```

产物：`target/release/herald-mcserver.exe`（~10 MB，含前端）

---

## 文档

- [部署指南](docs/DEPLOY.md)

## 许可证

- [GPL-3.0-only](LICENSE)

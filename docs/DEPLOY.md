# 部署指南

- 请注意，该工具是为了便于AI工作流调试，请不要接入任何生产环境。它是个测试工具不是一个正式环境的组件。
- 此外，安全相关不做任何保证和相关维护（由于本工具仅用于AI调试，大部分代码是Vibe Coding，并没有经过过多人工审查）。

## Windows

```powershell
# 1. 把 herald-mcserver.exe 和 config.toml 放同一目录
# 2. 编辑 config.toml
# 3. 运行
.\herald-mcserver.exe --config config.toml
```

打开 http://localhost:8787 完成首次设置。

**后台常驻**（PowerShell）：

```powershell
Start-Process -NoNewWindow .\herald-mcserver.exe -ArgumentList "--config","config.toml"
```

停止则关闭窗口。

---

## Linux

### 直接润

```bash
chmod +x herald-mcserver
./herald-mcserver --config config.toml
```

### systemd 托管

创建 `/etc/systemd/system/herald-mcserver.service`：

```ini
[Unit]
Description=Herald MCServerMCP
After=network.target

[Service]
Type=simple
User=herald
WorkingDirectory=/opt/herald-mcserver
ExecStart=/opt/herald-mcserver/herald-mcserver --config config.toml
Restart=on-failure
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

```bash
sudo useradd --system herald
sudo mkdir -p /opt/herald-mcserver
# 把 exe + config.toml 放进去

sudo systemctl daemon-reload
sudo systemctl enable --now herald-mcserver
sudo journalctl -u herald-mcserver -f   # 看日志
```

### nginx HTTPS 反代

```nginx
server {
    listen 443 ssl http2;
    server_name mcs.example.com;

    client_max_body_size 80m;

    location /ws {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 1h;
    }

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

用 `certbot --nginx -d mcs.example.com` 加 HTTPS。

此时 config.toml 应设：
```toml
[server]
listen      = "127.0.0.1:8787"   # 绑 loopback，nginx 反代
public_url  = "https://mcs.example.com"
public_host = "mcs.example.com"

[security]
trusted_proxy = "127.0.0.1/32"
```

---

## 防火墙

| 端口          | 开放 | 用途                |
|-------------|----|-------------------|
| 8787（或 443） | 是  | Web 面板 + MCP      |
| 25565       | 按需 | MC 客户端连入测试        |
| 25575       | 否  | RCON（内部 loopback） |

---

## 配置要点

```toml
[server]
listen      = "0.0.0.0:8787"    # 测试机直接绑全接口；生产绑 127.0.0.1 走 nginx
public_host = "你的IP或域名"     # AI 工具从这拿到连接地址

[mc]
server_port = 25565              # MC 端口，每次启动强制写入 server.properties
heap_mb     = 4096               # Paper JVM 内存

[runtime]
mirror = "default"               # 中国大陆用 "bmclapi" 或 "tuna" 加速下载
```

---

## 备份

数据都在 `data_dir`（默认 `./data`）：

- `auth.db` — 账号/Key/审计（sqlite，可用 `.backup` 命令热备）
- `server/default/` — Paper 世界/插件/配置

JRE 和 Paper jar 缓存可以重下载，不需要备份。

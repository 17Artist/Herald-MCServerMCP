import { useEffect, useState } from "react";
import {
  keys as keysApi,
  ApiError,
  type ApiKeyDto,
  type ApiKeyScope,
  type ApiKeyCreateResp,
  type McpEndpointResp,
} from "../lib/api";

export function ApiKeysPanel() {
  const [list, setList] = useState<ApiKeyDto[] | null>(null);
  const [endpoint, setEndpoint] = useState<McpEndpointResp | null>(null);
  const [err, setErr] = useState<string | null>(null);
  /** 新创建的 key —— 显示一次明文，刷新后消失。 */
  const [revealed, setRevealed] = useState<ApiKeyCreateResp | null>(null);

  async function refresh() {
    setErr(null);
    try {
      const [l, e] = await Promise.all([keysApi.list(), keysApi.endpoint()]);
      setList(l);
      setEndpoint(e);
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  }

  useEffect(() => { void refresh(); }, []);

  return (
    <div className="space-y-5">
      <header>
        <h2 className="text-lg font-semibold text-ink-100">MCP Keys</h2>
        <p className="text-xs text-ink-400 mt-0.5">
          给 AI 编程助手用的 API Key。Claude Desktop / Cursor / Cline 等 MCP 客户端通过 Bearer 接入。
        </p>
      </header>

      {err && <div className="alert-error">{err}</div>}

      <EndpointBlock endpoint={endpoint} />

      <CreateForm
        onCreated={(c) => {
          setRevealed(c);
          void refresh();
        }}
      />

      {revealed && (
        <RevealedSecret resp={revealed} endpoint={endpoint} onDismiss={() => setRevealed(null)} />
      )}

      <KeyList list={list} onChanged={() => void refresh()} />
    </div>
  );
}

function EndpointBlock({ endpoint }: { endpoint: McpEndpointResp | null }) {
  if (!endpoint) return null;
  return (
    <div className="glass rounded-xl p-4 text-xs">
      <div className="flex items-baseline gap-3 mb-1.5">
        <span className="text-ink-500 w-16 shrink-0">MCP URL</span>
        <code className="text-violet-200 font-mono">{endpoint.mcp_url}</code>
        <button
          className="btn-ghost text-[10px] py-0.5 px-2 ml-auto"
          onClick={() => navigator.clipboard.writeText(endpoint.mcp_url)}
        >
          复制
        </button>
      </div>
      <div className="flex items-baseline gap-3">
        <span className="text-ink-500 w-16 shrink-0">协议</span>
        <span className="text-ink-200">MCP Streamable HTTP（POST /mcp，仅 application/json）</span>
      </div>
      <div className="flex items-baseline gap-3 mt-1">
        <span className="text-ink-500 w-16 shrink-0">状态</span>
        {endpoint.mcp_enabled
          ? <span className="text-emerald-300">● 已启用</span>
          : <span className="text-amber-300">○ 已禁用（config.toml [mcp].enabled=false）</span>}
      </div>
      <p className="text-[11px] text-ink-500 mt-3 leading-relaxed">
        提示：本服务用 cookie 鉴权，浏览器侧 MCP 走不通；MCP 客户端必须使用下方 API Key（Bearer）。
      </p>
    </div>
  );
}

function CreateForm({
  onCreated,
}: {
  onCreated: (resp: ApiKeyCreateResp) => void;
}) {
  const [name, setName] = useState("");
  const [scope, setScope] = useState<ApiKeyScope>("mcp:full");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function go() {
    if (!name.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      const resp = await keysApi.create(name.trim(), scope);
      setName("");
      onCreated(resp);
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="glass rounded-xl p-4">
      <h3 className="text-sm font-medium text-ink-100 mb-3">创建新 Key</h3>
      <div className="flex flex-wrap gap-2 items-end">
        <label className="field flex-1 min-w-[200px]">
          <span>名称</span>
          <input
            className="input"
            placeholder="例如 claude-desktop / 工作笔记本"
            value={name}
            maxLength={64}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label className="field w-[180px]">
          <span>权限</span>
          <select
            className="input"
            value={scope}
            onChange={(e) => setScope(e.target.value as ApiKeyScope)}
          >
            <option value="mcp:full">mcp:full（全部工具）</option>
            <option value="mcp:read">mcp:read（仅只读工具）</option>
          </select>
        </label>
        <button
          className="btn-primary text-xs"
          disabled={busy || !name.trim()}
          onClick={() => void go()}
        >
          {busy ? "创建中…" : "创建"}
        </button>
      </div>
      {err && <div className="alert-error mt-3">{err}</div>}
    </div>
  );
}

function RevealedSecret({
  resp,
  endpoint,
  onDismiss,
}: {
  resp: ApiKeyCreateResp;
  endpoint: McpEndpointResp | null;
  onDismiss: () => void;
}) {
  const [tab, setTab] = useState<"raw" | "claude" | "cursor" | "vscode">("claude");
  const url = endpoint?.mcp_url ?? "";
  const cfg = mcpConfigs(resp.key.name, url, resp.secret);

  function copyText(text: string) {
    navigator.clipboard.writeText(text);
  }

  return (
    <div className="rounded-xl border border-violet-500/40 bg-violet-500/5 p-4 space-y-4">
      <div>
        <div className="text-sm font-medium text-violet-100">
          ● 新 Key 已创建：{resp.key.name}
        </div>
        <div className="text-[11px] text-amber-300 mt-1">
          ⚠ 明文 secret 仅展示一次。关闭此卡片后将无法再次查看，丢失只能吊销重发。
        </div>
      </div>

      <div className="text-xs">
        <div className="flex items-center gap-2 mb-1.5">
          <span className="text-ink-500 w-16 shrink-0">Secret</span>
          <code className="font-mono text-ink-100 select-all break-all">{resp.secret}</code>
          <button className="btn-ghost text-[10px] py-0.5 px-2 ml-auto" onClick={() => copyText(resp.secret)}>
            复制
          </button>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-ink-500 w-16 shrink-0">Scope</span>
          <span className="text-ink-200">{resp.key.scope}</span>
        </div>
      </div>

      <div>
        <div className="flex items-center gap-1 border-b border-violet-500/20 mb-2">
          <TabBtn active={tab === "claude"} onClick={() => setTab("claude")}>
            Claude Desktop
          </TabBtn>
          <TabBtn active={tab === "cursor"} onClick={() => setTab("cursor")}>
            Cursor
          </TabBtn>
          <TabBtn active={tab === "vscode"} onClick={() => setTab("vscode")}>
            VS Code (Cline)
          </TabBtn>
          <TabBtn active={tab === "raw"} onClick={() => setTab("raw")}>
            通用 JSON
          </TabBtn>
        </div>

        <ConfigBlock
          json={tab === "claude" ? cfg.claude : tab === "cursor" ? cfg.cursor : tab === "vscode" ? cfg.vscode : cfg.raw}
          hint={
            tab === "claude"
              ? "粘贴到 ~/Library/Application Support/Claude/claude_desktop_config.json（macOS）或 %APPDATA%\\Claude\\claude_desktop_config.json（Windows）。"
              : tab === "cursor"
                ? "粘贴到 ~/.cursor/mcp.json（用户级）或项目根目录的 .cursor/mcp.json。"
                : tab === "vscode"
                  ? "粘贴到 .vscode/mcp.json 或 Cline 的 settings → MCP Servers。"
                  : "通用形态。`url` 是 Streamable HTTP 端点，`headers.Authorization` 带 Bearer 前缀。"
          }
        />
      </div>

      <div className="flex justify-end">
        <button className="btn-ghost text-xs" onClick={onDismiss}>
          我已保存好 Secret，关闭
        </button>
      </div>
    </div>
  );
}

function TabBtn({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`text-xs px-3 py-1.5 -mb-px border-b-2 transition-colors ${
        active
          ? "border-violet-400 text-violet-100"
          : "border-transparent text-ink-400 hover:text-ink-200"
      }`}
    >
      {children}
    </button>
  );
}

function ConfigBlock({ json, hint }: { json: string; hint: string }) {
  return (
    <div>
      <div className="relative">
        <pre className="text-[11px] font-mono bg-ink-950 border border-ink-800 rounded-md p-3 overflow-auto leading-relaxed max-h-[260px]">
{json}
        </pre>
        <button
          className="btn-ghost text-[10px] py-0.5 px-2 absolute top-2 right-2"
          onClick={() => navigator.clipboard.writeText(json)}
        >
          复制
        </button>
      </div>
      <p className="text-[11px] text-ink-500 mt-2">{hint}</p>
    </div>
  );
}

/**
 * 输出三种主流 MCP 客户端的 JSON 配置。
 *
 * 命名空间用 `herald-mcserver`（去横线后是 `mcserver`，避免和别人项目重名）。
 * Streamable HTTP 形态在不同 client 里字段名略有差异：
 *   - Claude Desktop  / Cursor / Cline 都支持 `url + headers` 的 HTTP transport
 *   - 通用 JSON 给只支持 stdio 的老 client 提示用 mcp-remote 桥接
 */
function mcpConfigs(label: string, url: string, secret: string) {
  const safeName = label.replace(/[^a-zA-Z0-9_-]/g, "-").toLowerCase() || "herald-mcserver";

  const httpEntry = {
    url,
    headers: { Authorization: `Bearer ${secret}` },
  };

  const claude = stringify({
    mcpServers: {
      [safeName]: httpEntry,
    },
  });

  const cursor = stringify({
    mcpServers: {
      [safeName]: httpEntry,
    },
  });

  // VS Code (Cline / Continue 等) 用 servers 命名空间。
  const vscode = stringify({
    servers: {
      [safeName]: {
        type: "http",
        url,
        headers: { Authorization: `Bearer ${secret}` },
      },
    },
  });

  // 通用 JSON：附 stdio 兼容指引（mcp-remote）。
  const raw = stringify({
    name: safeName,
    transport: "streamable-http",
    url,
    headers: { Authorization: `Bearer ${secret}` },
    _comment_for_stdio_clients:
      "若客户端仅支持 stdio，可用 npx -y mcp-remote " + url + " --header \"Authorization: Bearer " + secret + "\" 桥接。",
  });

  return { claude, cursor, vscode, raw };
}

function stringify(obj: unknown): string {
  return JSON.stringify(obj, null, 2);
}

function KeyList({
  list,
  onChanged,
}: {
  list: ApiKeyDto[] | null;
  onChanged: () => void;
}) {
  if (!list) return <div className="text-sm text-ink-400">加载中…</div>;
  if (list.length === 0) {
    return <p className="text-xs text-ink-500 italic">尚无 Key —— 用上面的表单创建一把。</p>;
  }
  return (
    <div className="space-y-2">
      {list.map((k) => <KeyRow key={k.id} k={k} onChanged={onChanged} />)}
    </div>
  );
}

function KeyRow({ k, onChanged }: { k: ApiKeyDto; onChanged: () => void }) {
  const created = new Date(k.created_at * 1000).toLocaleString();
  const lastUsed = k.last_used_at
    ? new Date(k.last_used_at * 1000).toLocaleString()
    : "—";
  const isRevoked = k.revoked_at != null;
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function revoke() {
    if (!confirm(`吊销 ${k.name}？吊销后此 Key 立即失效，操作不可恢复。`)) return;
    setBusy(true);
    setErr(null);
    try {
      await keysApi.revoke(k.id);
      onChanged();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={`px-3 py-2 rounded-md border text-xs ${
      isRevoked
        ? "border-ink-800 bg-ink-900/30 opacity-70"
        : "border-ink-800 bg-ink-900/60"
    }`}>
      <div className="flex items-center gap-3">
        <span className="text-ink-100 font-medium truncate">{k.name}</span>
        <span className={`px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wider border ${
          k.scope === "mcp:full"
            ? "bg-violet-500/15 text-violet-300 border-violet-500/30"
            : "bg-sky-500/15 text-sky-300 border-sky-500/30"
        }`}>
          {k.scope}
        </span>
        {isRevoked && (
          <span className="px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wider bg-red-500/15 text-red-300 border border-red-500/30">
            已吊销
          </span>
        )}
        <span className="ml-auto text-ink-500">最近使用：{lastUsed}</span>
        {!isRevoked && (
          <button className="btn-ghost text-[11px] py-1 px-2" disabled={busy} onClick={() => void revoke()}>
            {busy ? "…" : "吊销"}
          </button>
        )}
      </div>
      <div className="text-[10px] text-ink-500 mt-1">id={k.id} · 创建于 {created}</div>
      {err && <div className="alert-error mt-2 text-[11px]">{err}</div>}
    </div>
  );
}

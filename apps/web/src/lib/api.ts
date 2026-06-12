/**
 * 极简 fetch 包装。后端错误体永远是 `{ error: string, message: string }`。
 * cookie 默认随请求带上（`credentials: include`）。
 */

export interface ApiErrorBody {
  error: string;
  message: string;
}

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, body: ApiErrorBody) {
    super(body.message || body.error || `HTTP ${status}`);
    this.status = status;
    this.code = body.error;
  }
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const init: RequestInit = {
    method,
    credentials: "include",
    headers: { "Accept": "application/json" },
  };
  if (body !== undefined) {
    init.headers = { ...init.headers, "Content-Type": "application/json" };
    init.body = JSON.stringify(body);
  }
  const resp = await fetch(path, init);
  if (resp.status === 204) return undefined as T;
  const text = await resp.text();
  let json: unknown = null;
  if (text) {
    try { json = JSON.parse(text); } catch { /* fallthrough */ }
  }
  if (!resp.ok) {
    const errBody: ApiErrorBody =
      json && typeof json === "object" && "error" in json
        ? (json as ApiErrorBody)
        : { error: "http", message: text || `HTTP ${resp.status}` };
    throw new ApiError(resp.status, errBody);
  }
  return (json ?? undefined) as T;
}

export const api = {
  get:  <T>(p: string) => request<T>("GET", p),
  post: <T>(p: string, body?: unknown) => request<T>("POST", p, body),
  del:  <T>(p: string) => request<T>("DELETE", p),
};

// ────────────────────────────────────────────────────────────────────────────
// Endpoints (S1)
// ────────────────────────────────────────────────────────────────────────────

export interface SetupState { initialized: boolean; }
export interface User { id: string; username: string; role: "owner" | "member"; }

export const auth = {
  setupState: () => api.get<SetupState>("/api/setup/state"),
  setupInit: (username: string, password: string) =>
    api.post<User>("/api/setup/init", { username, password }),
  login: (username: string, password: string, device?: string) =>
    api.post<User>("/api/auth/login", { username, password, device }),
  logout: () => api.del<void>("/api/auth/session"),
  me: () => api.get<User>("/api/auth/me"),
};

// ────────────────────────────────────────────────────────────────────────────
// Endpoints (S2)
// ────────────────────────────────────────────────────────────────────────────

export interface JavaInfo {
  path: string;
  major: number;
  vendor: string | null;
  source: "JAVA_HOME" | "PATH" | "managed" | "config";
}

export interface CachedPaper {
  version: string;
  build: number;
  jar_path: string;
  size: number;
}

export interface ProbeResp {
  os: string;
  arch: string;
  javas: JavaInfo[];
  managed_jdks: string[];
  paper_cache: CachedPaper[];
  default_mc_version: string;
  need_java_major_for_default: number;
}

export type TaskKind = "installjava" | "installpaper";
export type TaskStatus = "queued" | "running" | "done" | "failed";

export interface TaskSnapshot {
  id: string;
  kind: TaskKind;
  label: string;
  status: TaskStatus;
  downloaded: number;
  total: number | null;
  error: string | null;
  started_at: number;
  finished_at: number | null;
}

export const env = {
  probe: () => api.get<ProbeResp>("/api/env/probe"),
  installJava: (major: number) =>
    api.post<{ task_id: string }>("/api/env/install/java", { major }),
  installPaper: (version: string, build?: number) =>
    api.post<{ task_id: string }>("/api/env/install/paper", { version, build }),
  listTasks: () => api.get<TaskSnapshot[]>("/api/env/tasks"),
  taskStatus: (id: string) => api.get<TaskSnapshot>(`/api/env/tasks/${id}`),
};

export type ServerStatus = "stopped" | "starting" | "running" | "stopping";

export interface ServerSnapshot {
  status: ServerStatus;
  pid: number | null;
  mc_version: string | null;
  started_at: number | null;
  work_dir: string | null;
}

export interface LogLine {
  ts: number;
  stream: "stdout" | "stderr";
  text: string;
}

export interface StartReq {
  mc_version?: string;
  heap_mb?: number;
  server_port?: number;
  rcon_port?: number;
  rcon_password?: string;
  wait_ready_secs?: number;
}

export const server = {
  status: () => api.get<ServerSnapshot>("/api/server/status"),
  start: (req?: StartReq) =>
    api.post<ServerSnapshot>("/api/server/start", req ?? {}),
  stop: (force = false) =>
    api.post<ServerSnapshot>("/api/server/stop", { force }),
  restart: (req?: StartReq) =>
    api.post<ServerSnapshot>("/api/server/restart", req ?? {}),
  logs: (tail = 200) =>
    api.get<{ lines: LogLine[] }>(`/api/server/logs?tail=${tail}`),
  exec: (command: string) =>
    api.post<void>("/api/server/exec", { command }),
};

// ────────────────────────────────────────────────────────────────────────────
// Endpoints (S3)
// ────────────────────────────────────────────────────────────────────────────

export interface PluginEntry {
  filename: string;
  size: number;
  modified_ts: number;
}
export interface PluginListResp {
  plugins_dir: string;
  entries: PluginEntry[];
}
export interface PluginUploadResp {
  filename: string;
  size: number;
  replaced: boolean;
}

export const plugins = {
  list: () => api.get<PluginListResp>("/api/plugins/list"),
  remove: (filename: string) => api.del<void>(`/api/plugins/${encodeURIComponent(filename)}`),
  upload: async (file: File, replace: boolean): Promise<PluginUploadResp> => {
    const fd = new FormData();
    fd.append("file", file, file.name);
    if (replace) fd.append("replace", "true");
    const resp = await fetch("/api/plugins/upload", {
      method: "POST",
      credentials: "include",
      body: fd,
    });
    const text = await resp.text();
    let json: unknown = null;
    try { json = text ? JSON.parse(text) : null; } catch { /* ignore */ }
    if (!resp.ok) {
      const body = (json && typeof json === "object" && "error" in json)
        ? (json as { error: string; message: string })
        : { error: "http", message: text || `HTTP ${resp.status}` };
      throw new ApiError(resp.status, body);
    }
    return json as PluginUploadResp;
  },
};

export interface FileEntry {
  path: string;
  exists: boolean;
  size: number;
}
export interface FileReadResp {
  path: string;
  exists: boolean;
  content: string;
  size: number;
}

export const files = {
  list: () => api.get<FileEntry[]>("/api/files/list"),
  read: (path: string) =>
    api.get<FileReadResp>(`/api/files/read?path=${encodeURIComponent(path)}`),
  write: (path: string, content: string) =>
    api.post<FileReadResp>("/api/files/write", { path, content }),
};

export interface RconEndpointResp {
  configured: boolean;
  host: string | null;
  port: number | null;
  password: string | null;
}
export interface RconExecResp {
  command: string;
  response: string;
}

export const rcon = {
  endpoint: () => api.get<RconEndpointResp>("/api/rcon/endpoint"),
  exec: (command: string) =>
    api.post<RconExecResp>("/api/rcon/exec", { command }),
};

// ────────────────────────────────────────────────────────────────────────────
// Endpoints (S4)
// ────────────────────────────────────────────────────────────────────────────

export type ApiKeyScope = "mcp:full" | "mcp:read";

export interface ApiKeyDto {
  id: string;
  name: string;
  scope: ApiKeyScope;
  created_at: number;
  last_used_at: number | null;
  revoked_at: number | null;
}

export interface ApiKeyCreateResp {
  key: ApiKeyDto;
  /** 明文，只露这一次。 */
  secret: string;
}

export interface McpEndpointResp {
  mcp_url: string;
  mcp_enabled: boolean;
}

export const keys = {
  list: () => api.get<ApiKeyDto[]>("/api/keys/list"),
  create: (name: string, scope: ApiKeyScope) =>
    api.post<ApiKeyCreateResp>("/api/keys/create", { name, scope }),
  revoke: (id: string) => api.del<void>(`/api/keys/${encodeURIComponent(id)}`),
  endpoint: () => api.get<McpEndpointResp>("/api/keys/endpoint"),
};

// ────────────────────────────────────────────────────────────────────────────
// MCP 活动 + S5 admin
// ────────────────────────────────────────────────────────────────────────────

export type ActivityStatus = "ok" | "error" | "forbidden";

export type McpActivityEvent =
  | {
      type: "start";
      id: string;
      tool: string;
      summary: string;
      key_name: string;
      scope: ApiKeyScope;
      ts: number;
    }
  | {
      type: "finish";
      id: string;
      tool: string;
      status: ActivityStatus;
      message: string | null;
      duration_ms: number;
      ts: number;
    };

export const activity = {
  list: () => api.get<McpActivityEvent[]>("/api/activity/list"),
};

export interface AdminUserDto {
  id: string;
  username: string;
  role: "owner" | "member";
  created_at: number;
}

export interface InviteDto {
  code: string;
  note: string | null;
  created_at: number;
  expires_at: number;
  consumed_at: number | null;
  consumed_by: string | null;
}

export interface AuditEntry {
  id: number;
  ts: number;
  kind: string;
  actor_id: string | null;
  actor_label: string | null;
  ok: boolean;
  detail: string | null;
}

export const admin = {
  listUsers: () => api.get<AdminUserDto[]>("/api/admin/users"),
  deleteUser: (id: string) =>
    api.del<void>(`/api/admin/users/${encodeURIComponent(id)}`),
  listInvites: () => api.get<InviteDto[]>("/api/admin/invites"),
  createInvite: (note?: string) =>
    api.post<InviteDto>("/api/admin/invites", { note: note ?? null }),
  revokeInvite: (code: string) =>
    api.del<void>(`/api/admin/invites/${encodeURIComponent(code)}`),
  audit: (limit = 200) =>
    api.get<AuditEntry[]>(`/api/admin/audit?limit=${limit}`),
};

export const authExtras = {
  redeem: (code: string, username: string, password: string, device?: string) =>
    api.post<User>("/api/auth/redeem", { code, username, password, device }),
};

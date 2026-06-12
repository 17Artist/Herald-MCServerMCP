import { create } from "zustand";
import {
  activity as activityApi,
  env,
  server,
  type ProbeResp,
  type ServerSnapshot,
  type LogLine,
  type TaskSnapshot,
  type McpActivityEvent,
  type ActivityStatus,
} from "./api";
import { WsClient } from "./ws";

/** 活动条目：合并了 start + finish 的双事件，前端按 id 关联。 */
export interface ActivityItem {
  id: string;
  tool: string;
  summary: string;
  key_name: string;
  scope: "mcp:full" | "mcp:read";
  started_at: number;
  /** 仍在运行 = null；完成后填 finish 时间。 */
  finished_at: number | null;
  status: ActivityStatus | "running";
  message: string | null;
  duration_ms: number | null;
}

interface WorkbenchState {
  ws: WsClient | null;

  serverSnap: ServerSnapshot | null;
  logs: LogLine[];

  envProbe: ProbeResp | null;
  envProbeLoading: boolean;

  tasks: Record<string, TaskSnapshot>;

  /** 活跃活动（仍在运行）—— 渲染顶部"AI 正在控这台服务器"动效条。 */
  activeMcp: Record<string, ActivityItem>;
  /** 历史时间线（含已完成）—— 倒序，最多 100 条。 */
  mcpHistory: ActivityItem[];

  init(): Promise<void>;
  refreshEnv(): Promise<void>;
  refreshServer(): Promise<void>;
  refreshLogs(): Promise<void>;
  destroy(): void;
}

const MAX_LOG_LINES = 4000;
const MAX_HISTORY = 100;

export const useWorkbench = create<WorkbenchState>((set, get) => ({
  ws: null,
  serverSnap: null,
  logs: [],
  envProbe: null,
  envProbeLoading: false,
  tasks: {},
  activeMcp: {},
  mcpHistory: [],

  async init() {
    if (get().ws) return;

    const ws = new WsClient();
    set({ ws });
    ws.connect();

    ws.subscribe((frame) => {
      if (frame.channel === "server") {
        const ev = frame.event;
        if (ev.type === "status_change") {
          set((s) => ({
            serverSnap: s.serverSnap
              ? { ...s.serverSnap, status: ev.status, pid: ev.pid }
              : { status: ev.status, pid: ev.pid, mc_version: null, started_at: null, work_dir: null },
          }));
          get().refreshServer().catch(() => {});
        } else if (ev.type === "log") {
          set((s) => {
            const next = [...s.logs, ev.line];
            if (next.length > MAX_LOG_LINES) next.splice(0, next.length - MAX_LOG_LINES);
            return { logs: next };
          });
        }
      } else if (frame.channel === "task") {
        const t = frame.event.task;
        set((s) => ({ tasks: { ...s.tasks, [t.id]: t } }));
        if (t.status === "done" || t.status === "failed") {
          get().refreshEnv().catch(() => {});
        }
      } else if (frame.channel === "mcp_activity") {
        applyActivity(set, frame.event);
      }
    });

    await Promise.all([
      get().refreshServer(),
      get().refreshEnv(),
      get().refreshLogs(),
      bootstrapActivity(set),
    ]);
  },

  async refreshEnv() {
    set({ envProbeLoading: true });
    try {
      const probe = await env.probe();
      const tasks = await env.listTasks();
      set({
        envProbe: probe,
        tasks: Object.fromEntries(tasks.map((t) => [t.id, t])),
      });
    } finally {
      set({ envProbeLoading: false });
    }
  },

  async refreshServer() {
    const snap = await server.status();
    set({ serverSnap: snap });
  },

  async refreshLogs() {
    const r = await server.logs(500);
    set({ logs: r.lines });
  },

  destroy() {
    get().ws?.close();
    set({ ws: null });
  },
}));

async function bootstrapActivity(
  set: (partial: Partial<WorkbenchState>) => void,
) {
  try {
    const events = await activityApi.list();
    if (events.length === 0) return;

    // 重放成 active map + history list（一次性 set，避免 N 次 re-render）
    events.sort((a, b) => a.ts - b.ts);
    const active: Record<string, ActivityItem> = {};
    const history: ActivityItem[] = [];
    for (const ev of events) {
      if (ev.type === "start") {
        const item: ActivityItem = {
          id: ev.id,
          tool: ev.tool,
          summary: ev.summary,
          key_name: ev.key_name,
          scope: ev.scope,
          started_at: ev.ts,
          finished_at: null,
          status: "running",
          message: null,
          duration_ms: null,
        };
        active[ev.id] = item;
        // 历史列表：插到头
        const idx = history.findIndex((h) => h.id === ev.id);
        if (idx >= 0) history.splice(idx, 1);
        history.unshift(item);
      } else {
        // finish —— 把对应 item 标完成
        delete active[ev.id];
        const idx = history.findIndex((h) => h.id === ev.id);
        if (idx >= 0) {
          history[idx] = {
            ...history[idx],
            status: ev.status,
            message: ev.message,
            duration_ms: ev.duration_ms,
            finished_at: ev.ts,
          };
        }
      }
    }
    if (history.length > MAX_HISTORY) history.length = MAX_HISTORY;
    set({ activeMcp: active, mcpHistory: history });
  } catch {
    /* swallow */
  }
}

function applyActivity(
  set: (fn: (s: WorkbenchState) => Partial<WorkbenchState>) => void,
  ev: McpActivityEvent,
) {
  if (ev.type === "start") {
    const item: ActivityItem = {
      id: ev.id,
      tool: ev.tool,
      summary: ev.summary,
      key_name: ev.key_name,
      scope: ev.scope,
      started_at: ev.ts,
      finished_at: null,
      status: "running",
      message: null,
      duration_ms: null,
    };
    set((s) => ({
      activeMcp: { ...s.activeMcp, [ev.id]: item },
      mcpHistory: prependHistory(s.mcpHistory, item),
    }));
    return;
  }
  // finish
  set((s) => {
    const next = { ...s.activeMcp };
    delete next[ev.id];
    return {
      activeMcp: next,
      mcpHistory: s.mcpHistory.map((h) =>
        h.id === ev.id
          ? {
              ...h,
              status: ev.status,
              message: ev.message,
              duration_ms: ev.duration_ms,
              finished_at: ev.ts,
            }
          : h,
      ),
    };
  });
}

function prependHistory(prev: ActivityItem[], item: ActivityItem): ActivityItem[] {
  // 倒序，新的在前；若 id 已存在（回放）则更新
  const filtered = prev.filter((p) => p.id !== item.id);
  filtered.unshift(item);
  if (filtered.length > MAX_HISTORY) filtered.length = MAX_HISTORY;
  return filtered;
}

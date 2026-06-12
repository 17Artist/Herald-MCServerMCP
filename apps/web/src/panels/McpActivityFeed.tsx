import { useEffect, useMemo, useState } from "react";
import { useWorkbench, type ActivityItem } from "../lib/workbench";

/**
 * AI 通过 MCP 操控服务器的"活动可视化条"。
 *
 * 三层呈现：
 *   1. 顶部活跃条：每个进行中的 tool 显示一个发光脉冲环 + 工具名 + 参数摘要
 *   2. 折叠/展开历史时间线（默认折叠，避免抢日志面板的注意）
 *   3. 完成的条目以柔和动画过渡到历史区
 */
export function McpActivityFeed() {
  const activeMap = useWorkbench((s) => s.activeMcp);
  const history = useWorkbench((s) => s.mcpHistory);
  const [expanded, setExpanded] = useState(false);

  // 用 useMemo 派生数组，避免每次 render 引用变化导致父组件死循环
  const active = useMemo(
    () => Object.values(activeMap).sort((a, b) => b.started_at - a.started_at),
    [activeMap],
  );

  if (active.length === 0 && history.length === 0) {
    // 还没有任何活动 —— 显示一个等待提示，让用户知道这里会出现什么
    return (
      <div className="rounded-xl border border-ink-800 bg-ink-900/30 px-4 py-3 text-xs text-ink-500 flex items-center gap-3">
        <span className="dot inline-block w-2 h-2 rounded-full bg-ink-700"></span>
        <span>
          AI 控制台空闲。
          <span className="ml-2 text-ink-400">
            一旦有 MCP 客户端通过 API Key 调用工具，会在此实时呈现。
          </span>
        </span>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {active.length > 0 && (
        <div className="space-y-1.5">
          {active.map((item) => <ActiveBar key={item.id} item={item} />)}
        </div>
      )}

      {(history.length > 0) && (
        <div>
          <button
            className="text-[11px] text-ink-400 hover:text-ink-200 flex items-center gap-1.5 transition-colors"
            onClick={() => setExpanded((v) => !v)}
          >
            <span className={`inline-block transition-transform ${expanded ? "rotate-90" : ""}`}>
              ▸
            </span>
            最近 MCP 调用 · {history.length}
          </button>
          {expanded && (
            <div className="mt-2 space-y-1 max-h-[320px] overflow-auto pr-1">
              {history.map((it) => <HistoryRow key={it.id} item={it} />)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ActiveBar({ item }: { item: ActivityItem }) {
  const elapsed = useElapsed(item.started_at);
  return (
    <div className="mcp-active-bar rounded-xl px-4 py-3 flex items-center gap-3 fade-in">
      <div className="mcp-pulse">
        <div className="core" />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2">
          <span className="text-[10px] uppercase tracking-wider text-violet-300/80 font-mono">
            AI MCP
          </span>
          <span className="mcp-tool-name text-sm font-mono">{item.tool}</span>
          <span className="text-[11px] text-ink-400">
            via <span className="text-ink-200">{item.key_name}</span>
          </span>
          <span className={`text-[10px] uppercase tracking-wider px-1.5 rounded border ${
            item.scope === "mcp:full"
              ? "bg-violet-500/15 text-violet-300 border-violet-500/30"
              : "bg-sky-500/15 text-sky-300 border-sky-500/30"
          }`}>
            {item.scope.split(":")[1]}
          </span>
        </div>
        {item.summary && (
          <div className="text-[11px] text-ink-300 font-mono mt-1 truncate" title={item.summary}>
            {item.summary}
          </div>
        )}
      </div>
      <div className="text-[11px] font-mono text-violet-200/70 shrink-0">
        {elapsed}
      </div>
    </div>
  );
}

function HistoryRow({ item }: { item: ActivityItem }) {
  const isOk = item.status === "ok";
  const isErr = item.status === "error" || item.status === "forbidden";
  const isRunning = item.status === "running";

  const dot = isRunning
    ? "bg-violet-400 animate-pulse"
    : isOk
      ? "bg-emerald-400"
      : "bg-red-400";

  const flashClass =
    item.finished_at != null && Date.now() - item.finished_at < 1500
      ? isOk ? "mcp-flash-ok" : isErr ? "mcp-flash-err" : ""
      : "";

  return (
    <div className={`mcp-history-item rounded-md border border-ink-800 bg-ink-900/40 px-3 py-1.5 ${flashClass}`}>
      <div className="flex items-center gap-2 text-[11px] font-mono">
        <span className={`w-1.5 h-1.5 rounded-full ${dot}`} />
        <span className="text-ink-100">{item.tool}</span>
        <span className="text-ink-500">·</span>
        <span className="text-ink-400">{item.key_name}</span>
        <span className="text-ink-500 ml-auto">
          {isRunning
            ? "running…"
            : item.duration_ms != null
              ? formatDuration(item.duration_ms)
              : "—"}
        </span>
        <time className="text-ink-600 text-[10px]">
          {new Date(item.started_at).toLocaleTimeString()}
        </time>
      </div>
      {item.summary && (
        <div className="text-[10.5px] text-ink-500 font-mono mt-0.5 truncate" title={item.summary}>
          {item.summary}
        </div>
      )}
      {item.message && isErr && (
        <div className="text-[10.5px] text-red-300 font-mono mt-0.5 truncate" title={item.message}>
          ⚠ {item.message}
        </div>
      )}
    </div>
  );
}

function useElapsed(startedAtMs: number): string {
  const [tick, setTick] = useState(Date.now());
  useEffect(() => {
    const id = setInterval(() => setTick(Date.now()), 100);
    return () => clearInterval(id);
  }, []);
  const ms = Math.max(0, tick - startedAtMs);
  return formatDuration(ms);
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
}

/** 给 Workbench 顶栏用的小指示器：有活跃调用时显示。 */
export function McpNavIndicator() {
  const activeCount = useWorkbench((s) => Object.keys(s.activeMcp).length);
  if (activeCount === 0) return null;
  return (
    <span className="mcp-nav-indicator">
      <span className="mcp-nav-dot" />
      AI 控制中 · {activeCount}
    </span>
  );
}

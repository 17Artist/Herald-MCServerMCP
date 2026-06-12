import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { useWorkbench } from "../lib/workbench";
import { server, ApiError, type LogLine, type ServerStatus } from "../lib/api";
import { McpActivityFeed } from "./McpActivityFeed";

const STATUS_LABEL: Record<ServerStatus, string> = {
  stopped: "已停止",
  starting: "启动中",
  running: "运行中",
  stopping: "停止中",
};

const STATUS_DOT: Record<ServerStatus, string> = {
  stopped: "bg-ink-500",
  starting: "bg-amber-400",
  running: "bg-emerald-400",
  stopping: "bg-amber-400",
};

export function ServerPanel() {
  const snap = useWorkbench((s) => s.serverSnap);
  const logs = useWorkbench((s) => s.logs);

  const status = snap?.status ?? "stopped";
  const busy = status === "starting" || status === "stopping";

  const [actionErr, setActionErr] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);

  async function runAction(fn: () => Promise<unknown>) {
    setActionErr(null);
    setActionBusy(true);
    try {
      await fn();
    } catch (e) {
      if (e instanceof ApiError) {
        setActionErr(e.message);
      } else {
        setActionErr(String(e));
      }
    } finally {
      setActionBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <McpActivityFeed />

      <header className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-ink-100">服务端</h2>
          <p className="text-xs text-ink-400 mt-0.5">
            {snap?.mc_version ? <>当前版本 <code className="text-ink-200">{snap.mc_version}</code></> : "尚未启动"}
            {snap?.pid ? <> · PID {snap.pid}</> : null}
          </p>
        </div>

        <div className="flex items-center gap-2 text-sm">
          <span className={`dot inline-block w-2 h-2 rounded-full ${STATUS_DOT[status]} ${busy ? "pulse-dot" : ""}`} />
          <span className="text-ink-200">{STATUS_LABEL[status]}</span>
        </div>
      </header>

      <div className="flex items-center gap-2">
        <button
          className="btn-primary text-xs"
          disabled={actionBusy || busy || status === "running"}
          onClick={() => runAction(() => server.start())}
        >
          启动
        </button>
        <button
          className="btn-ghost text-xs"
          disabled={actionBusy || busy || status === "stopped"}
          onClick={() => runAction(() => server.stop())}
        >
          停止
        </button>
        <button
          className="btn-ghost text-xs"
          disabled={actionBusy || busy || status === "stopped"}
          onClick={() => runAction(() => server.restart())}
        >
          重启
        </button>
      </div>

      {actionErr && <ErrorCard message={actionErr} />}

      <ConsoleTerm logs={logs} disabled={status !== "running"} />
    </div>
  );
}

function ErrorCard({ message }: { message: string }) {
  let parsed: Record<string, unknown> | null = null;
  try {
    parsed = JSON.parse(message);
  } catch { /* not JSON */ }

  // env_missing 给个引导提示
  if (parsed && (parsed as any).code === "env_missing") {
    const need = (parsed as any).need_java_major as number | undefined;
    const have = (parsed as any).have_java as number | null | undefined;
    const paperCached = (parsed as any).paper_cached as boolean | undefined;
    return (
      <div className="alert-error">
        <div className="font-medium mb-1">无法启动：环境未就绪</div>
        <ul className="text-[12px] list-disc pl-4 leading-relaxed">
          {need != null && (
            <li>
              需要 Java {need}+，当前 {have == null ? "未发现可用 Java" : `仅有 Java ${have}`}。
              请到 <span className="text-violet-300">环境管家</span> 面板下载。
            </li>
          )}
          {paperCached === false && (
            <li>未缓存 Paper jar。请到 <span className="text-violet-300">环境管家</span> 面板下载。</li>
          )}
        </ul>
      </div>
    );
  }
  return <div className="alert-error whitespace-pre-wrap break-words">{message}</div>;
}

function ConsoleTerm({ logs, disabled }: { logs: LogLine[]; disabled: boolean }) {
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const lastSeenLenRef = useRef(0);
  const [cmd, setCmd] = useState("");
  const [sending, setSending] = useState(false);

  useEffect(() => {
    if (!wrapRef.current) return;
    const term = new Terminal({
      fontFamily: '"JetBrains Mono", "Cascadia Code", ui-monospace, monospace',
      fontSize: 12,
      theme: {
        background: "#0a0a0c",
        foreground: "#e4e4e7",
        cursor: "#a78bfa",
        selectionBackground: "rgba(167,139,250,0.25)",
        black: "#27272d",
        red: "#fca5a5",
        green: "#86efac",
        yellow: "#fcd34d",
        blue: "#7dd3fc",
        magenta: "#c4b5fd",
        cyan: "#67e8f9",
        white: "#e4e4e7",
      },
      convertEol: true,
      scrollback: 5000,
      disableStdin: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(wrapRef.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;

    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(wrapRef.current);

    return () => {
      ro.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, []);

  // 增量打印新行
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;

    if (logs.length < lastSeenLenRef.current) {
      // 日志被裁过 / 切了实例 —— 全清重写。
      term.clear();
      lastSeenLenRef.current = 0;
    }
    for (let i = lastSeenLenRef.current; i < logs.length; i++) {
      const line = logs[i];
      const prefix = line.stream === "stderr" ? "\x1b[31m" : "";
      const suffix = line.stream === "stderr" ? "\x1b[0m" : "";
      term.writeln(`${prefix}${line.text}${suffix}`);
    }
    lastSeenLenRef.current = logs.length;
  }, [logs]);

  async function send() {
    const c = cmd.trim();
    if (!c || sending) return;
    setSending(true);
    try {
      await server.exec(c);
      setCmd("");
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      termRef.current?.writeln(`\x1b[31m[执行失败] ${msg}\x1b[0m`);
    } finally {
      setSending(false);
    }
  }

  return (
    <div>
      <div className="border border-ink-800 rounded-lg overflow-hidden bg-[#0a0a0c]">
        <div ref={wrapRef} className="h-[420px] px-2 py-2" />
      </div>
      <div className="flex gap-2 mt-3">
        <input
          className="input font-mono text-xs"
          placeholder={disabled ? "（服务未运行，无法发送命令）" : "console 命令，如 list / op artist / time set day"}
          value={cmd}
          onChange={(e) => setCmd(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void send();
          }}
          disabled={disabled || sending}
        />
        <button
          className="btn-primary text-xs whitespace-nowrap"
          onClick={() => void send()}
          disabled={disabled || sending || !cmd.trim()}
        >
          发送
        </button>
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { rcon as rconApi, ApiError, type RconEndpointResp, type RconExecResp } from "../lib/api";
import { useWorkbench } from "../lib/workbench";

interface HistoryEntry {
  id: number;
  command: string;
  response: string;
  ok: boolean;
}

export function RconPanel() {
  const snap = useWorkbench((s) => s.serverSnap);
  const running = snap?.status === "running";

  const [endpoint, setEndpoint] = useState<RconEndpointResp | null>(null);
  const [reveal, setReveal] = useState(false);
  const [cmd, setCmd] = useState("");
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<HistoryEntry[]>([]);

  useEffect(() => {
    rconApi.endpoint().then(setEndpoint).catch(() => setEndpoint(null));
  }, [running]);

  async function send() {
    const c = cmd.trim();
    if (!c || busy) return;
    setBusy(true);
    try {
      const r: RconExecResp = await rconApi.exec(c);
      setHistory((h) => [
        { id: Date.now(), command: r.command, response: r.response || "（空回复）", ok: true },
        ...h.slice(0, 49),
      ]);
      setCmd("");
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      setHistory((h) => [
        { id: Date.now(), command: c, response: msg, ok: false },
        ...h.slice(0, 49),
      ]);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <header>
        <h2 className="text-lg font-semibold text-ink-100">RCON</h2>
        <p className="text-xs text-ink-400 mt-0.5">
          通过 RCON 协议发命令，直接拿到回包文本。比 console 更适合脚本化调试。
        </p>
      </header>

      <div className="glass rounded-xl p-4 text-xs">
        {endpoint?.configured ? (
          <div className="space-y-1.5">
            <Row label="状态" value={running ? "● 已就绪" : "○ 已配置但服务未运行"} accent={running ? "emerald" : "amber"} />
            <Row label="主机" value={endpoint.host ?? "—"} />
            <Row label="端口" value={endpoint.port?.toString() ?? "—"} />
            <Row
              label="密码"
              value={
                <span className="flex items-center gap-2">
                  <code className="font-mono">
                    {reveal ? endpoint.password : "•".repeat(Math.min(24, (endpoint.password ?? "").length))}
                  </code>
                  <button className="btn-ghost text-[10px] py-0.5 px-2" onClick={() => setReveal((v) => !v)}>
                    {reveal ? "隐藏" : "显示"}
                  </button>
                  <button
                    className="btn-ghost text-[10px] py-0.5 px-2"
                    onClick={() => endpoint.password && navigator.clipboard.writeText(endpoint.password)}
                  >
                    复制
                  </button>
                </span>
              }
            />
            <p className="text-[11px] text-ink-500 mt-2">
              密码由服务端在每次启动时随机生成；停服后失效。RCON 仅监听 127.0.0.1。
            </p>
          </div>
        ) : (
          <div className="text-ink-400">RCON 端点尚未配置 —— 请先启动服务。</div>
        )}
      </div>

      <div>
        <div className="flex gap-2">
          <input
            className="input font-mono text-xs"
            placeholder={running ? "RCON 命令，如 list / save-all / time set day" : "（服务未运行，无法发送）"}
            value={cmd}
            onChange={(e) => setCmd(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void send();
            }}
            disabled={!running || busy}
          />
          <button
            className="btn-primary text-xs whitespace-nowrap"
            disabled={!running || busy || !cmd.trim()}
            onClick={() => void send()}
          >
            发送
          </button>
        </div>
        <p className="text-[11px] text-ink-500 mt-1.5">
          单条上限 1400 字节；长命令请用 console（服务端面板）。
        </p>
      </div>

      <div className="space-y-2">
        {history.length === 0 ? (
          <p className="text-xs text-ink-500 italic">尚无 RCON 历史。</p>
        ) : (
          history.map((h) => (
            <div
              key={h.id}
              className={`px-3 py-2 rounded-md border text-xs ${
                h.ok
                  ? "border-ink-800 bg-ink-900/60"
                  : "border-red-500/30 bg-red-500/5"
              }`}
            >
              <div className="font-mono text-violet-300">› {h.command}</div>
              <pre className="font-mono whitespace-pre-wrap text-ink-200 mt-1">{h.response}</pre>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function Row({
  label,
  value,
  accent,
}: {
  label: string;
  value: React.ReactNode;
  accent?: "emerald" | "amber";
}) {
  const colorCls = accent === "emerald" ? "text-emerald-300" : accent === "amber" ? "text-amber-300" : "text-ink-200";
  return (
    <div className="flex items-baseline gap-3">
      <span className="text-ink-500 w-12 shrink-0">{label}</span>
      <span className={colorCls}>{value}</span>
    </div>
  );
}

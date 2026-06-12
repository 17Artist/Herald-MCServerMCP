import { useState } from "react";
import { useWorkbench } from "../lib/workbench";
import { env as envApi, ApiError, type CachedPaper, type JavaInfo, type TaskSnapshot } from "../lib/api";

export function EnvironmentPanel() {
  const probe = useWorkbench((s) => s.envProbe);
  const loading = useWorkbench((s) => s.envProbeLoading);
  const tasks = useWorkbench((s) => s.tasks);
  const refresh = useWorkbench((s) => s.refreshEnv);

  if (!probe && loading) return <Loading label="正在探测环境…" />;
  if (!probe) return <Loading label="环境信息暂不可用" />;

  const need = probe.need_java_major_for_default;
  const hasJavaForDefault = probe.javas.some((j) => j.major >= need);
  const cachedForDefault = probe.paper_cache.some(
    (p) => p.version === probe.default_mc_version,
  );

  const activeTasks = Object.values(tasks)
    .filter((t) => t.status === "running" || t.status === "queued")
    .sort((a, b) => b.started_at - a.started_at);

  return (
    <div className="space-y-5">
      <header className="flex items-baseline justify-between">
        <div>
          <h2 className="text-lg font-semibold text-ink-100">环境管家</h2>
          <p className="text-xs text-ink-400 mt-0.5">
            主机 <code className="text-ink-200">{probe.os} / {probe.arch}</code> ·
            默认 MC <code className="text-ink-200">{probe.default_mc_version}</code> 需要 Java {need}+
          </p>
        </div>
        <button className="btn-ghost text-xs" onClick={() => refresh()}>
          刷新
        </button>
      </header>

      {activeTasks.length > 0 && (
        <section>
          <h3 className="text-sm font-medium text-ink-200 mb-2">进行中任务</h3>
          <div className="space-y-2">
            {activeTasks.map((t) => <TaskRow key={t.id} task={t} />)}
          </div>
        </section>
      )}

      <section>
        <h3 className="text-sm font-medium text-ink-200 mb-2">Java 运行时</h3>

        <StatusLine
          ok={hasJavaForDefault}
          okText={`已具备 Java ${need}+`}
          missText={`默认 MC 需要 Java ${need}，当前未发现满足要求的版本`}
        />

        <div className="mt-3 space-y-1.5">
          {probe.javas.length === 0 ? (
            <p className="text-xs text-ink-500">未检测到任何 Java。</p>
          ) : (
            probe.javas.map((j, i) => <JavaRow key={i} java={j} />)
          )}
        </div>

        {!hasJavaForDefault && (
          <InstallJava major={need} />
        )}
      </section>

      <section>
        <h3 className="text-sm font-medium text-ink-200 mb-2">PaperMC jar</h3>

        <StatusLine
          ok={cachedForDefault}
          okText={`已缓存 ${probe.default_mc_version}`}
          missText={`未缓存 ${probe.default_mc_version}`}
        />

        <div className="mt-3 space-y-1.5">
          {probe.paper_cache.length === 0 ? (
            <p className="text-xs text-ink-500">尚无任何 Paper jar。</p>
          ) : (
            probe.paper_cache.map((p) => <PaperRow key={p.jar_path} paper={p} />)
          )}
        </div>

        {!cachedForDefault && (
          <InstallPaper version={probe.default_mc_version} />
        )}
      </section>
    </div>
  );
}

function Loading({ label }: { label: string }) {
  return <div className="text-sm text-ink-400 fade-in">{label}</div>;
}

function StatusLine({ ok, okText, missText }: { ok: boolean; okText: string; missText: string }) {
  return (
    <p className={`text-xs ${ok ? "text-emerald-300" : "text-amber-300"}`}>
      {ok ? "● " : "○ "}{ok ? okText : missText}
    </p>
  );
}

function JavaRow({ java }: { java: JavaInfo }) {
  return (
    <div className="text-xs flex items-center gap-3 px-3 py-2 rounded-md border border-ink-800 bg-ink-900/60">
      <span className="px-1.5 py-0.5 rounded bg-violet-500/15 text-violet-300 text-[10px] uppercase tracking-wider border border-violet-500/20">
        Java {java.major}
      </span>
      <span className="text-ink-300 truncate" title={java.path}>{java.path}</span>
      <span className="ml-auto text-ink-500">{java.vendor ?? "Unknown"} · {java.source}</span>
    </div>
  );
}

function PaperRow({ paper }: { paper: CachedPaper }) {
  const mb = (paper.size / (1024 * 1024)).toFixed(1);
  return (
    <div className="text-xs flex items-center gap-3 px-3 py-2 rounded-md border border-ink-800 bg-ink-900/60">
      <span className="px-1.5 py-0.5 rounded bg-sky-500/15 text-sky-300 text-[10px] uppercase tracking-wider border border-sky-500/20">
        {paper.version}
      </span>
      <span className="text-ink-300">build {paper.build}</span>
      <span className="ml-auto text-ink-500">{mb} MB</span>
    </div>
  );
}

function InstallJava({ major }: { major: number }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function go() {
    setBusy(true);
    setErr(null);
    try {
      await envApi.installJava(major);
    } catch (e) {
      if (e instanceof ApiError) setErr(e.message);
      else setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-3">
      <button className="btn-primary text-xs" disabled={busy} onClick={go}>
        {busy ? "正在排队…" : `下载 Java ${major}（Adoptium Temurin JRE）`}
      </button>
      {err && <p className="alert-error mt-2">{err}</p>}
    </div>
  );
}

function InstallPaper({ version }: { version: string }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function go() {
    setBusy(true);
    setErr(null);
    try {
      await envApi.installPaper(version);
    } catch (e) {
      if (e instanceof ApiError) setErr(e.message);
      else setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-3">
      <button className="btn-primary text-xs" disabled={busy} onClick={go}>
        {busy ? "正在排队…" : `下载 Paper ${version}（最新构建）`}
      </button>
      {err && <p className="alert-error mt-2">{err}</p>}
    </div>
  );
}

function TaskRow({ task }: { task: TaskSnapshot }) {
  const pct =
    task.total && task.total > 0
      ? Math.min(100, Math.round((task.downloaded / task.total) * 100))
      : null;
  const sizeText = task.total
    ? `${(task.downloaded / 1024 / 1024).toFixed(1)} / ${(task.total / 1024 / 1024).toFixed(1)} MB`
    : `${(task.downloaded / 1024 / 1024).toFixed(1)} MB`;

  return (
    <div className="px-3 py-2 rounded-md border border-violet-500/30 bg-violet-500/5">
      <div className="flex items-center justify-between text-xs">
        <span className="text-ink-100">{task.label}</span>
        <span className="text-ink-400">{sizeText}{pct != null ? ` · ${pct}%` : ""}</span>
      </div>
      <div className="h-1.5 bg-ink-800 rounded-full mt-1.5 overflow-hidden">
        <div
          className="h-full bg-gradient-to-r from-violet-400 to-sky-400 transition-all duration-300"
          style={{ width: pct != null ? `${pct}%` : "100%", opacity: pct != null ? 1 : 0.4 }}
        />
      </div>
    </div>
  );
}

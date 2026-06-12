import { useEffect, useState } from "react";
import { files as filesApi, ApiError, type FileEntry } from "../lib/api";

export function FilesPanel() {
  const [list, setList] = useState<FileEntry[] | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function refreshList() {
    setErr(null);
    try {
      const items = await filesApi.list();
      setList(items);
      // 默认打开 server.properties
      if (!active && items.length > 0) {
        setActive(items[0].path);
      }
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  }

  useEffect(() => { void refreshList(); }, []);

  return (
    <div className="space-y-4">
      <header>
        <h2 className="text-lg font-semibold text-ink-100">配置文件</h2>
        <p className="text-xs text-ink-400 mt-0.5">
          白名单：server.properties / ops.json / whitelist.json / banned-* / paper-* / bukkit.yml / spigot.yml
        </p>
      </header>

      {err && <div className="alert-error">{err}</div>}

      {list && (
        <div className="grid grid-cols-[200px_1fr] gap-4 min-h-[420px]">
          <div className="space-y-1">
            {list.map((f) => (
              <button
                key={f.path}
                onClick={() => setActive(f.path)}
                className={`w-full text-left text-xs px-3 py-2 rounded-md border transition-colors ${
                  active === f.path
                    ? "bg-violet-500/15 text-violet-100 border-violet-500/30"
                    : "bg-ink-900/40 text-ink-200 border-ink-800 hover:bg-ink-800/60"
                }`}
              >
                <div className="truncate">{f.path}</div>
                <div className="text-[10px] mt-0.5 text-ink-500">
                  {f.exists ? `${f.size} B` : "尚未生成"}
                </div>
              </button>
            ))}
          </div>

          {active && <FileEditor path={active} onSaved={() => void refreshList()} />}
        </div>
      )}
    </div>
  );
}

function FileEditor({ path, onSaved }: { path: string; onSaved: () => void }) {
  const [content, setContent] = useState("");
  const [original, setOriginal] = useState("");
  const [exists, setExists] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setErr(null);
    setInfo(null);
    filesApi
      .read(path)
      .then((r) => {
        if (cancelled) return;
        setContent(r.content);
        setOriginal(r.content);
        setExists(r.exists);
      })
      .catch((e) => {
        if (cancelled) return;
        setErr(e instanceof ApiError ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, [path]);

  const dirty = content !== original;

  async function save() {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      const r = await filesApi.write(path, content);
      setOriginal(r.content);
      setExists(true);
      setInfo("已保存");
      onSaved();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  if (loading) return <div className="text-sm text-ink-400">读取中…</div>;

  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-2 mb-2 text-xs text-ink-400">
        <code className="text-ink-200">{path}</code>
        {!exists && <span className="text-amber-300">（尚未生成 · 保存后会创建）</span>}
        {dirty && <span className="text-violet-300">● 未保存</span>}
        <span className="ml-auto">{content.length} B</span>
      </div>
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        spellCheck={false}
        className="flex-1 min-h-[400px] font-mono text-[12px] leading-relaxed input"
        style={{ resize: "vertical", whiteSpace: "pre", overflowWrap: "normal", overflowX: "auto" }}
      />
      <div className="flex gap-2 mt-3 items-center">
        <button className="btn-primary text-xs" disabled={busy || !dirty} onClick={() => void save()}>
          {busy ? "保存中…" : "保存"}
        </button>
        <button
          className="btn-ghost text-xs"
          disabled={busy || !dirty}
          onClick={() => setContent(original)}
        >
          放弃修改
        </button>
        {info && <span className="text-xs text-emerald-300">{info}</span>}
        {err && <span className="text-xs text-red-300">{err}</span>}
        <span className="text-[11px] text-ink-500 ml-auto">部分配置仅在重启服务后生效</span>
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import {
  plugins as pluginsApi,
  ApiError,
  type PluginEntry,
  type PluginListResp,
} from "../lib/api";

export function PluginsPanel() {
  const [data, setData] = useState<PluginListResp | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);

  async function refresh() {
    setLoading(true);
    setErr(null);
    try {
      setData(await pluginsApi.list());
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void refresh(); }, []);

  return (
    <div className="space-y-5">
      <header className="flex items-baseline justify-between">
        <div>
          <h2 className="text-lg font-semibold text-ink-100">插件</h2>
          <p className="text-xs text-ink-400 mt-0.5">
            部署到 <code className="text-ink-200">{data?.plugins_dir ?? "plugins/"}</code> · 插件加载/卸载需要重启
          </p>
        </div>
        <button className="btn-ghost text-xs" onClick={() => void refresh()}>刷新</button>
      </header>

      <Uploader onDone={() => void refresh()} />

      {err && <div className="alert-error">{err}</div>}

      {loading && !data && <div className="text-sm text-ink-400">加载中…</div>}

      {data && (
        <div className="space-y-2">
          {data.entries.length === 0 ? (
            <p className="text-xs text-ink-500 italic">尚无插件 jar。把文件拖到上方虚框里上传。</p>
          ) : (
            data.entries.map((p) => (
              <PluginRow key={p.filename} entry={p} onChanged={() => void refresh()} />
            ))
          )}
        </div>
      )}
    </div>
  );
}

function Uploader({ onDone }: { onDone: () => void }) {
  const [drag, setDrag] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  async function uploadFiles(files: FileList | File[]) {
    setErr(null);
    setInfo(null);
    setBusy(true);
    try {
      let success = 0;
      for (const f of Array.from(files)) {
        if (!f.name.toLowerCase().endsWith(".jar")) {
          throw new Error(`${f.name} 不是 .jar 文件`);
        }
        try {
          await pluginsApi.upload(f, /* replace */ true);
          success += 1;
        } catch (e) {
          if (e instanceof ApiError) throw e;
          throw e;
        }
      }
      setInfo(`已上传 ${success} 个 jar`);
      onDone();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <label
        onDragOver={(e) => { e.preventDefault(); setDrag(true); }}
        onDragLeave={() => setDrag(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDrag(false);
          if (e.dataTransfer.files.length) void uploadFiles(e.dataTransfer.files);
        }}
        className={`block text-center cursor-pointer transition-colors rounded-xl border-2 border-dashed px-6 py-8 ${
          drag
            ? "border-violet-400 bg-violet-500/10"
            : "border-ink-700 hover:border-ink-600 bg-ink-900/30"
        }`}
      >
        <input
          type="file"
          accept=".jar"
          multiple
          className="hidden"
          disabled={busy}
          onChange={(e) => {
            if (e.target.files?.length) void uploadFiles(e.target.files);
            e.target.value = "";
          }}
        />
        <div className="text-sm text-ink-200">
          {busy ? "正在上传…" : "拖拽 .jar 到此 · 或点击选择文件"}
        </div>
        <div className="text-xs text-ink-500 mt-1">
          会校验 ZIP 头 + plugin.yml / paper-plugin.yml；同名文件直接覆盖
        </div>
      </label>

      {info && <div className="text-xs text-emerald-300 mt-2">● {info}</div>}
      {err && <div className="alert-error mt-2">{err}</div>}
    </div>
  );
}

function PluginRow({ entry, onChanged }: { entry: PluginEntry; onChanged: () => void }) {
  const kb = entry.size / 1024;
  const sizeText = kb >= 1024 ? `${(kb / 1024).toFixed(1)} MB` : `${kb.toFixed(0)} KB`;
  const ts = entry.modified_ts
    ? new Date(entry.modified_ts * 1000).toLocaleString()
    : "—";

  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function remove() {
    if (!confirm(`确认删除 ${entry.filename}？删除后需要重启服务端才生效。`)) return;
    setBusy(true);
    setErr(null);
    try {
      await pluginsApi.remove(entry.filename);
      onChanged();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="px-3 py-2 rounded-md border border-ink-800 bg-ink-900/60">
      <div className="flex items-center gap-3 text-xs">
        <span className="text-ink-100 font-medium truncate">{entry.filename}</span>
        <span className="text-ink-500">{sizeText}</span>
        <span className="text-ink-500 ml-auto">{ts}</span>
        <button className="btn-ghost text-[11px] py-1 px-2" disabled={busy} onClick={() => void remove()}>
          {busy ? "…" : "删除"}
        </button>
      </div>
      {err && <div className="alert-error mt-2 text-[11px]">{err}</div>}
    </div>
  );
}

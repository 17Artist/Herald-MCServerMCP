import { useEffect, useState } from "react";
import {
  admin,
  ApiError,
  type AdminUserDto,
  type InviteDto,
  type AuditEntry,
} from "../lib/api";

type Tab = "members" | "invites" | "audit";

export function AdminPanel() {
  const [tab, setTab] = useState<Tab>("members");

  return (
    <div className="space-y-4">
      <header>
        <h2 className="text-lg font-semibold text-ink-100">管理</h2>
        <p className="text-xs text-ink-400 mt-0.5">
          仅 Owner 可见。维护成员、邀请码、操作审计日志。
        </p>
      </header>

      <div className="flex gap-1 border-b border-ink-800">
        <SubTab active={tab === "members"} onClick={() => setTab("members")}>成员</SubTab>
        <SubTab active={tab === "invites"} onClick={() => setTab("invites")}>邀请码</SubTab>
        <SubTab active={tab === "audit"} onClick={() => setTab("audit")}>审计日志</SubTab>
      </div>

      {tab === "members" && <MembersTab />}
      {tab === "invites" && <InvitesTab />}
      {tab === "audit" && <AuditTab />}
    </div>
  );
}

function SubTab({
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

function MembersTab() {
  const [users, setUsers] = useState<AdminUserDto[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function refresh() {
    setErr(null);
    try {
      setUsers(await admin.listUsers());
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  }

  useEffect(() => { void refresh(); }, []);

  async function remove(u: AdminUserDto) {
    if (!confirm(`删除成员 ${u.username}？此操作不可撤回。`)) return;
    try {
      await admin.deleteUser(u.id);
      void refresh();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  }

  if (!users) return <div className="text-sm text-ink-400">加载中…</div>;

  return (
    <div className="space-y-2">
      {err && <div className="alert-error">{err}</div>}
      {users.map((u) => (
        <div key={u.id} className="px-3 py-2 rounded-md border border-ink-800 bg-ink-900/60 flex items-center gap-3 text-xs">
          <span className="text-ink-100 font-medium">{u.username}</span>
          <span className={`px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wider border ${
            u.role === "owner"
              ? "bg-violet-500/15 text-violet-300 border-violet-500/30"
              : "bg-sky-500/15 text-sky-300 border-sky-500/30"
          }`}>
            {u.role}
          </span>
          <span className="text-ink-500 ml-auto">
            注册于 {new Date(u.created_at * 1000).toLocaleString()}
          </span>
          {u.role !== "owner" && (
            <button
              className="btn-ghost text-[11px] py-1 px-2"
              onClick={() => void remove(u)}
            >
              删除
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

function InvitesTab() {
  const [list, setList] = useState<InviteDto[] | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [recentlyCreated, setRecentlyCreated] = useState<InviteDto | null>(null);

  async function refresh() {
    setErr(null);
    try {
      setList(await admin.listInvites());
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  }

  useEffect(() => { void refresh(); }, []);

  async function create() {
    setBusy(true);
    setErr(null);
    try {
      const inv = await admin.createInvite(note.trim() || undefined);
      setRecentlyCreated(inv);
      setNote("");
      void refresh();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function revoke(code: string) {
    if (!confirm(`吊销邀请码 ${code}？`)) return;
    try {
      await admin.revokeInvite(code);
      void refresh();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  }

  return (
    <div className="space-y-3">
      <div className="glass rounded-xl p-4">
        <h3 className="text-sm font-medium text-ink-100 mb-3">创建邀请码</h3>
        <div className="flex gap-2 items-end">
          <label className="field flex-1">
            <span>备注（可选）</span>
            <input
              className="input"
              placeholder="例如 给 fellow-dev / 测试机器人"
              value={note}
              maxLength={64}
              onChange={(e) => setNote(e.target.value)}
            />
          </label>
          <button className="btn-primary text-xs" disabled={busy} onClick={() => void create()}>
            {busy ? "生成中…" : "生成"}
          </button>
        </div>
        <p className="text-[11px] text-ink-500 mt-2">默认 24 小时过期，使用一次后失效。</p>
      </div>

      {recentlyCreated && (
        <div className="rounded-xl border border-violet-500/40 bg-violet-500/5 p-4">
          <div className="text-sm text-violet-100 mb-2">● 邀请码已生成</div>
          <div className="flex items-center gap-2 text-xs">
            <code className="text-ink-100 font-mono text-base select-all">{recentlyCreated.code}</code>
            <button
              className="btn-ghost text-[10px] py-0.5 px-2 ml-auto"
              onClick={() => navigator.clipboard.writeText(recentlyCreated.code)}
            >
              复制
            </button>
          </div>
          <p className="text-[11px] text-ink-400 mt-2">
            把这串码发给受邀方，让 ta 访问登录页 → 切到"用邀请码注册"输入即可。
          </p>
        </div>
      )}

      {err && <div className="alert-error">{err}</div>}

      {list && (
        <div className="space-y-2">
          {list.length === 0 ? (
            <p className="text-xs text-ink-500 italic">尚无邀请码记录。</p>
          ) : (
            list.map((inv) => <InviteRow key={inv.code} invite={inv} onRevoke={() => void revoke(inv.code)} />)
          )}
        </div>
      )}
    </div>
  );
}

function InviteRow({ invite, onRevoke }: { invite: InviteDto; onRevoke: () => void }) {
  const now = Date.now() / 1000;
  const expired = invite.expires_at <= now;
  const consumed = invite.consumed_at != null;
  const status = consumed ? "已使用" : expired ? "已过期" : "可用";
  const cls = consumed
    ? "text-ink-500 border-ink-800 bg-ink-900/30"
    : expired
      ? "text-amber-300 border-amber-500/30 bg-amber-500/5"
      : "text-emerald-300 border-emerald-500/30 bg-emerald-500/5";

  return (
    <div className={`px-3 py-2 rounded-md border text-xs ${cls}`}>
      <div className="flex items-center gap-3">
        <code className="font-mono text-ink-100 select-all">{invite.code}</code>
        <span className="px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wider border-current border">
          {status}
        </span>
        {invite.note && <span className="text-ink-400 truncate">备注：{invite.note}</span>}
        <span className="ml-auto text-ink-500">
          过期：{new Date(invite.expires_at * 1000).toLocaleString()}
        </span>
        {!consumed && !expired && (
          <button
            className="btn-ghost text-[11px] py-1 px-2"
            onClick={onRevoke}
          >
            吊销
          </button>
        )}
      </div>
    </div>
  );
}

function AuditTab() {
  const [list, setList] = useState<AuditEntry[] | null>(null);

  async function refresh() {
    try {
      setList(await admin.audit(200));
    } catch (e) {
      console.error(e);
    }
  }

  useEffect(() => {
    void refresh();
    const id = setInterval(refresh, 10000);
    return () => clearInterval(id);
  }, []);

  if (!list) return <div className="text-sm text-ink-400">加载中…</div>;

  return (
    <div className="space-y-1.5">
      <p className="text-[11px] text-ink-500">
        最近 200 条 · 每 10 秒刷新一次。涵盖登录 / 邀请 / 用户删除等敏感操作。
      </p>
      {list.length === 0 ? (
        <p className="text-xs text-ink-500 italic">尚无审计记录。</p>
      ) : (
        list.map((e) => (
          <div
            key={e.id}
            className={`px-3 py-1.5 rounded-md border font-mono text-[11px] flex items-center gap-3 ${
              e.ok
                ? "border-ink-800 bg-ink-900/40"
                : "border-red-500/30 bg-red-500/5"
            }`}
          >
            <time className="text-ink-500 shrink-0">
              {new Date(e.ts * 1000).toLocaleTimeString()}
            </time>
            <span className={e.ok ? "text-violet-300" : "text-red-300"}>{e.kind}</span>
            <span className="text-ink-400">{e.actor_label ?? e.actor_id ?? "—"}</span>
            <span className={`px-1.5 rounded text-[10px] ${e.ok ? "bg-emerald-500/10 text-emerald-300" : "bg-red-500/15 text-red-300"}`}>
              {e.ok ? "ok" : "fail"}
            </span>
            {e.detail && <span className="text-ink-500 truncate ml-auto">{e.detail}</span>}
          </div>
        ))
      )}
    </div>
  );
}

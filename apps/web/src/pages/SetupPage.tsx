import { useEffect, useState } from "react";
import { Logo } from "../assets/Logo";
import { auth, ApiError } from "../lib/api";
import { useSession } from "../lib/session";

/**
 * 首次启动向导。仅在 `setup.state.initialized === false` 时显示。
 *
 * 提交规则：
 *   * 用户名 2-32 字符，仅 ASCII 字母/数字/下划线/横线/点
 *   * 密码 ≥ 8 字符
 *   * 两次密码必须一致
 */
export function SetupPage() {
  const setReady = useSession((s) => s.setReady);

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    document.title = "首次设置 · Herald MCServerMCP";
  }, []);

  const valid =
    username.trim().length >= 2 &&
    password.length >= 8 &&
    password === confirm &&
    !busy;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setErr(null);
    if (password !== confirm) {
      setErr("两次输入的密码不一致");
      return;
    }
    setBusy(true);
    try {
      const user = await auth.setupInit(username.trim(), password);
      setReady(user);
    } catch (e) {
      if (e instanceof ApiError) setErr(e.message);
      else setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <CenteredCard title="首次设置" subtitle="创建管理员账户后即可开始使用">
      <form onSubmit={submit} className="flex flex-col gap-4">
        <label className="field">
          <span>管理员用户名</span>
          <input
            className="input"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoFocus
            autoComplete="username"
            minLength={2}
            maxLength={32}
            placeholder="2-32 字符 · 字母/数字/._-"
            required
          />
        </label>
        <label className="field">
          <span>密码</span>
          <input
            className="input"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            minLength={8}
            autoComplete="new-password"
            placeholder="至少 8 个字符"
            required
          />
        </label>
        <label className="field">
          <span>再输一次密码</span>
          <input
            className="input"
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            minLength={8}
            autoComplete="new-password"
            required
          />
        </label>

        {err && <div className="alert-error">{err}</div>}

        <button type="submit" className="btn-primary mt-2" disabled={!valid}>
          {busy ? "创建中…" : "创建管理员"}
        </button>

        <p className="text-xs text-ink-400 mt-1 leading-relaxed">
          提示：管理员是唯一有权创建/吊销其他账号的角色。请妥善保存此密码。
          忘记密码时可使用 <code className="text-ink-200">--reset-owner-key</code> 启动选项重置（详见 README）。
        </p>
      </form>
    </CenteredCard>
  );
}

export function CenteredCard({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-full grid place-items-center px-4 py-12 fade-in">
      <div className="w-full max-w-md glass rounded-2xl shadow-glow p-8">
        <div className="flex items-center gap-3 mb-6">
          <Logo size={36} />
          <div className="leading-tight">
            <div className="text-[15px] font-semibold tracking-wide text-ink-100">
              Herald MCServerMCP
            </div>
            <div className="text-[11px] text-ink-400">
              Minecraft Plugin Debug Bridge
            </div>
          </div>
        </div>
        <h1 className="text-lg font-semibold text-ink-100">{title}</h1>
        {subtitle && (
          <p className="text-sm text-ink-400 mt-1 mb-5">{subtitle}</p>
        )}
        {children}
      </div>
    </div>
  );
}

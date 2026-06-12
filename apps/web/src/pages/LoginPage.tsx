import { useEffect, useState } from "react";
import { auth, authExtras, ApiError } from "../lib/api";
import { useSession } from "../lib/session";
import { getDeviceId } from "../lib/device";
import { CenteredCard } from "./SetupPage";

type Mode = "login" | "redeem";

export function LoginPage() {
  const setReady = useSession((s) => s.setReady);

  const [mode, setMode] = useState<Mode>("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    document.title = mode === "redeem"
      ? "邀请码注册 · Herald MCServerMCP"
      : "登录 · Herald MCServerMCP";
  }, [mode]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setErr(null);
    setBusy(true);
    try {
      if (mode === "login") {
        const user = await auth.login(username.trim(), password, getDeviceId());
        setReady(user);
      } else {
        const user = await authExtras.redeem(code.trim(), username.trim(), password, getDeviceId());
        setReady(user);
      }
    } catch (e) {
      if (e instanceof ApiError) setErr(e.message);
      else setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  const valid =
    mode === "login"
      ? username.trim().length >= 2 && password.length >= 1 && !busy
      : code.trim().length >= 4 && username.trim().length >= 2 && password.length >= 8 && !busy;

  return (
    <CenteredCard
      title={mode === "login" ? "登录" : "邀请码注册"}
      subtitle={mode === "login"
        ? "使用管理员或受邀成员账号登录"
        : "使用 Owner 给的一次性邀请码创建你的账户"}
    >
      <div className="flex gap-1 mb-4 border-b border-ink-800">
        <button
          type="button"
          onClick={() => setMode("login")}
          className={`text-xs px-3 py-1.5 -mb-px border-b-2 transition-colors ${
            mode === "login"
              ? "border-violet-400 text-violet-100"
              : "border-transparent text-ink-400 hover:text-ink-200"
          }`}
        >
          登录
        </button>
        <button
          type="button"
          onClick={() => setMode("redeem")}
          className={`text-xs px-3 py-1.5 -mb-px border-b-2 transition-colors ${
            mode === "redeem"
              ? "border-violet-400 text-violet-100"
              : "border-transparent text-ink-400 hover:text-ink-200"
          }`}
        >
          用邀请码注册
        </button>
      </div>

      <form onSubmit={submit} className="flex flex-col gap-4">
        {mode === "redeem" && (
          <label className="field">
            <span>邀请码</span>
            <input
              className="input font-mono"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              maxLength={32}
              placeholder="向 Owner 索要"
              autoFocus
              required
            />
          </label>
        )}
        <label className="field">
          <span>用户名</span>
          <input
            className="input"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoFocus={mode === "login"}
            autoComplete="username"
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
            autoComplete={mode === "login" ? "current-password" : "new-password"}
            minLength={mode === "redeem" ? 8 : undefined}
            placeholder={mode === "redeem" ? "至少 8 个字符" : undefined}
            required
          />
        </label>

        {err && <div className="alert-error">{err}</div>}

        <button type="submit" className="btn-primary mt-2" disabled={!valid}>
          {busy
            ? mode === "login" ? "登录中…" : "创建中…"
            : mode === "login" ? "登录" : "创建账户"}
        </button>
      </form>
    </CenteredCard>
  );
}

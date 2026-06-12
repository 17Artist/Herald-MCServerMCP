import { useEffect, useState } from "react";
import { Logo } from "../assets/Logo";
import { useSession } from "../lib/session";
import { useWorkbench } from "../lib/workbench";
import { EnvironmentPanel } from "../panels/EnvironmentPanel";
import { ServerPanel } from "../panels/ServerPanel";
import { PluginsPanel } from "../panels/PluginsPanel";
import { FilesPanel } from "../panels/FilesPanel";
import { RconPanel } from "../panels/RconPanel";
import { ApiKeysPanel } from "../panels/ApiKeysPanel";
import { AdminPanel } from "../panels/AdminPanel";
import { McpNavIndicator } from "../panels/McpActivityFeed";

type Tab = "server" | "environment" | "plugins" | "files" | "rcon" | "keys" | "admin";

export function WorkbenchPage() {
  const phase = useSession((s) => s.phase);
  const signOut = useSession((s) => s.signOut);

  const init = useWorkbench((s) => s.init);
  const destroy = useWorkbench((s) => s.destroy);

  const [tab, setTab] = useState<Tab>("server");

  useEffect(() => {
    void init();
    return () => destroy();
    // 仅启动期初始化一次。phase 变 logout 由 App 切走整页。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (phase.kind !== "ready") return null;
  const user = phase.user;

  return (
    <div className="min-h-full grid grid-cols-[220px_1fr] grid-rows-[auto_1fr] fade-in">
      <header className="col-span-2 border-b border-ink-800/80 px-5 py-3 flex items-center gap-3">
        <Logo size={28} />
        <div className="leading-tight">
          <div className="text-sm font-semibold text-ink-100">Herald MCServerMCP</div>
          <div className="text-[11px] text-ink-400">Plugin debug bridge · Stage 5</div>
        </div>
        <McpNavIndicator />
        <div className="ml-auto flex items-center gap-3 text-xs text-ink-300">
          <span>
            {user.username}
            <span className="ml-1.5 px-1.5 py-0.5 rounded bg-ink-800 text-[10px] uppercase tracking-wider text-ink-300">
              {user.role}
            </span>
          </span>
          <button className="btn-ghost text-xs" onClick={signOut}>登出</button>
        </div>
      </header>

      <nav className="border-r border-ink-800/80 p-3 flex flex-col gap-1 text-sm">
        <NavItem active={tab === "server"} onClick={() => setTab("server")} label="服务端" hint="启停 / 控制台" />
        <NavItem active={tab === "environment"} onClick={() => setTab("environment")} label="环境管家" hint="Java / Paper" />
        <NavItem active={tab === "plugins"} onClick={() => setTab("plugins")} label="插件" hint="上传 / 管理" />
        <NavItem active={tab === "files"} onClick={() => setTab("files")} label="文件" hint="配置编辑" />
        <NavItem active={tab === "rcon"} onClick={() => setTab("rcon")} label="RCON" hint="协议命令" />
        <NavItem active={tab === "keys"} onClick={() => setTab("keys")} label="MCP Keys" hint="AI 接入" />
        {user.role === "owner" && (
          <NavItem active={tab === "admin"} onClick={() => setTab("admin")} label="管理" hint="成员 / 审计" />
        )}
      </nav>

      <main className="overflow-auto p-6">
        {tab === "server" && <ServerPanel />}
        {tab === "environment" && <EnvironmentPanel />}
        {tab === "plugins" && <PluginsPanel />}
        {tab === "files" && <FilesPanel />}
        {tab === "rcon" && <RconPanel />}
        {tab === "keys" && <ApiKeysPanel />}
        {tab === "admin" && user.role === "owner" && <AdminPanel />}
      </main>
    </div>
  );
}

function NavItem({
  active,
  disabled,
  onClick,
  label,
  hint,
}: {
  active?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  label: string;
  hint?: string;
}) {
  if (disabled) {
    return (
      <div className="px-3 py-2 rounded-md text-ink-500 cursor-not-allowed flex items-center gap-2">
        <span>{label}</span>
        {hint && (
          <span className="ml-auto text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded bg-ink-800 text-ink-500">
            {hint}
          </span>
        )}
      </div>
    );
  }
  return (
    <button
      onClick={onClick}
      className={`px-3 py-2 rounded-md text-left flex items-center gap-2 transition-colors ${
        active
          ? "bg-violet-500/15 text-violet-100 border border-violet-500/30"
          : "text-ink-200 hover:bg-ink-800/60 border border-transparent"
      }`}
    >
      <span>{label}</span>
      {hint && (
        <span className={`ml-auto text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded ${
          active ? "bg-violet-500/20 text-violet-200" : "bg-ink-800 text-ink-400"
        }`}>
          {hint}
        </span>
      )}
    </button>
  );
}

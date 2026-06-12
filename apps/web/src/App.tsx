import { useEffect } from "react";
import { useSession } from "./lib/session";
import { SetupPage } from "./pages/SetupPage";
import { LoginPage } from "./pages/LoginPage";
import { WorkbenchPage } from "./pages/WorkbenchPage";

export function App() {
  const phase = useSession((s) => s.phase);
  const bootstrap = useSession((s) => s.bootstrap);

  useEffect(() => { void bootstrap(); }, [bootstrap]);

  if (phase.kind === "loading") return <Splash />;
  if (phase.kind === "setup")   return <SetupPage />;
  if (phase.kind === "login")   return <LoginPage />;
  return <WorkbenchPage />;
}

function Splash() {
  return (
    <div className="min-h-full grid place-items-center text-ink-400 text-sm">
      正在连接服务…
    </div>
  );
}

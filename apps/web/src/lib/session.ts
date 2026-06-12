import { create } from "zustand";
import { auth, ApiError, type User } from "./api";

type Phase =
  | { kind: "loading" }
  | { kind: "setup" }            // DB 空，强制走首次设置
  | { kind: "login" }            // 已有 owner，但当前未登录
  | { kind: "ready"; user: User };

interface SessionStore {
  phase: Phase;
  /** 启动时探测一次 setup state + me，决定首屏路由。 */
  bootstrap(): Promise<void>;
  /** 已经在 setup/login 页面手动设置：标记下一阶段。 */
  setReady(user: User): void;
  signOut(): Promise<void>;
}

export const useSession = create<SessionStore>((set) => ({
  phase: { kind: "loading" },

  async bootstrap() {
    try {
      const st = await auth.setupState();
      if (!st.initialized) {
        set({ phase: { kind: "setup" } });
        return;
      }
      // 已初始化 → 试一下 /me；401 则跳 login。
      try {
        const me = await auth.me();
        set({ phase: { kind: "ready", user: me } });
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) {
          set({ phase: { kind: "login" } });
          return;
        }
        throw e;
      }
    } catch (e) {
      console.error("bootstrap failed", e);
      // 网络挂了或后端没起 —— 退到 login 页让用户看见错误提示。
      set({ phase: { kind: "login" } });
    }
  },

  setReady(user) {
    set({ phase: { kind: "ready", user } });
  },

  async signOut() {
    try { await auth.logout(); } catch { /* 无所谓 */ }
    set({ phase: { kind: "login" } });
  },
}));

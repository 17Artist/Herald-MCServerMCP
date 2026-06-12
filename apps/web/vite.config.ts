import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 后端默认监听 :8787。开发时跑 `npm run dev` → vite :5173 代理到后端。
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:8787",
      "/ws":  { target: "ws://127.0.0.1:8787", ws: true },
      "/mcp": "http://127.0.0.1:8787",
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    chunkSizeWarningLimit: 1000,
  },
});

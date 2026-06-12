/**
 * 简易 WebSocket 客户端：自动重连 + 拆 frame 给订阅者。
 *
 * 后端的 frame 用 `tag = "channel"` 形式：
 *   { "channel": "hello",  user: "...",   role: "owner" }
 *   { "channel": "server", event: ServerEvent }    ← event.type 是子枚举的标签
 *   { "channel": "task",   event: TaskEvent }
 *   { "channel": "mcp_activity", event: McpActivityEvent }
 *   { "channel": "bye",    reason: "..." }
 */

import type { LogLine, ServerStatus, TaskSnapshot } from "./api";

export type WsFrame =
  | { channel: "hello"; user: string; role: string }
  | { channel: "server"; event: ServerEvent }
  | { channel: "task"; event: TaskEvent }
  | { channel: "mcp_activity"; event: McpActivityEvent }
  | { channel: "bye"; reason: string };

export type ServerEvent =
  | { type: "status_change"; status: ServerStatus; pid: number | null }
  | { type: "log"; line: LogLine };

export type TaskEvent = { type: "snapshot"; task: TaskSnapshot };

export type ActivityStatus = "ok" | "error" | "forbidden";

export type McpActivityEvent =
  | {
      type: "start";
      id: string;
      tool: string;
      summary: string;
      key_name: string;
      scope: "mcp:full" | "mcp:read";
      ts: number;
    }
  | {
      type: "finish";
      id: string;
      tool: string;
      status: ActivityStatus;
      message: string | null;
      duration_ms: number;
      ts: number;
    };

type Listener = (frame: WsFrame) => void;

export class WsClient {
  private ws: WebSocket | null = null;
  private listeners = new Set<Listener>();
  private retryDelay = 1000;
  private closed = false;

  connect() {
    if (this.closed) return;
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${location.host}/ws`;
    const ws = new WebSocket(url);
    this.ws = ws;

    ws.onopen = () => {
      this.retryDelay = 1000;
    };
    ws.onmessage = (ev) => {
      try {
        const raw = JSON.parse(ev.data) as WsFrame;
        this.dispatch(raw);
      } catch (e) {
        console.warn("ws bad frame", e, ev.data);
      }
    };
    ws.onclose = () => {
      this.ws = null;
      if (this.closed) return;
      const d = Math.min(this.retryDelay, 10000);
      this.retryDelay = Math.min(this.retryDelay * 1.6, 10000);
      setTimeout(() => this.connect(), d);
    };
    ws.onerror = () => { /* onclose 会接着触发，这里不重复处理 */ };
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private dispatch(frame: WsFrame) {
    for (const l of this.listeners) {
      try { l(frame); } catch (e) { console.error("ws listener", e); }
    }
  }
}

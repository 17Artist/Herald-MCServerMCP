/**
 * 一个稳定的 device id，用来在登录时 hint 给后端，使"同账号同设备"挤掉旧 session。
 * 仅本地标识，不参与认证。
 */
const KEY = "mcs_device_id";

function generateId(): string {
  // 兼容旧浏览器（crypto.randomUUID 需要 Chrome 92+）
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  // fallback: 用 getRandomValues 拼一个
  const buf = new Uint8Array(16);
  crypto.getRandomValues(buf);
  const hex = Array.from(buf, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function getDeviceId(): string {
  let v = localStorage.getItem(KEY);
  if (!v) {
    v = generateId();
    localStorage.setItem(KEY, v);
  }
  return v;
}

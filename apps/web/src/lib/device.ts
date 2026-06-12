/**
 * 一个稳定的 device id，用来在登录时 hint 给后端，使"同账号同设备"挤掉旧 session。
 * 仅本地标识，不参与认证。
 */
const KEY = "mcs_device_id";

export function getDeviceId(): string {
  let v = localStorage.getItem(KEY);
  if (!v) {
    v = crypto.randomUUID();
    localStorage.setItem(KEY, v);
  }
  return v;
}

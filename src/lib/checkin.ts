//! 入住机管理接口（7271 HTTP）前端直连封装
//!
//! 设备端已启用 CORS（含 OPTIONS 预检），浏览器/WebView 直连无需代理。
//! 登录失败限流：连续 5 次失败锁定 60s（HTTP 429）。

import type { CheckinDeviceInfo } from "@/types";

const TIMEOUT_MS = 6000;

/** 管理接口基地址 */
export function checkinBase(device: { ip: string; port: number }): string {
  return `http://${device.ip}:${device.port}`;
}

/** 管理台页面地址（免鉴权静态入口） */
export function checkinAdminUrl(device: { ip: string; port: number }): string {
  return `${checkinBase(device)}/admin/`;
}

/** 登录取 token（30 分钟有效，滑动续期） */
export async function checkinLogin(base: string, password: string): Promise<string> {
  const res = await fetch(`${base}/api/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password }),
    signal: AbortSignal.timeout(TIMEOUT_MS),
  });
  if (res.status === 429) throw new Error("连续失败已锁定，请 60 秒后重试");
  const body = (await res.json().catch(() => null)) as
    | { success?: boolean; token?: string }
    | null;
  if (!res.ok || !body?.success || !body.token) throw new Error("管理密码错误");
  return body.token;
}

/** 拉取设备详情；token 失效时抛出 TOKEN_EXPIRED，由调用方清除会话重新登录 */
export async function checkinDeviceInfo(
  base: string,
  token: string
): Promise<CheckinDeviceInfo> {
  const res = await fetch(`${base}/api/device/info`, {
    headers: { Authorization: `Bearer ${token}` },
    signal: AbortSignal.timeout(TIMEOUT_MS),
  });
  if (res.status === 401) throw new Error("TOKEN_EXPIRED");
  if (!res.ok) throw new Error(`获取设备信息失败（HTTP ${res.status}）`);
  return (await res.json()) as CheckinDeviceInfo;
}

/** 服务探活（免鉴权 GET /info），用于离线兜底判断 */
export async function checkinProbe(base: string): Promise<boolean> {
  try {
    const res = await fetch(`${base}/info`, { signal: AbortSignal.timeout(3000) });
    return res.ok;
  } catch {
    return false;
  }
}

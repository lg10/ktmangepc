import { getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

/** macOS：原生 overlay 标题栏（圆角/阴影/真红绿灯）；其他平台自定义无边框（自绘三键，标题栏即自定义内容栏） */
export const isMac = /Mac/i.test(navigator.userAgent);

/** IDEA 式启动检查小窗 */
export async function setSizeSplash() {
  const win = getCurrentWindow();
  await win.setResizable(false);
  await win.setSize(new LogicalSize(480, 344));
  await win.center();
}

/** 登录窗 */
export async function setSizeLogin() {
  const win = getCurrentWindow();
  // 工作台窗有 1080×700 最小尺寸限制，变登录窗前先解除
  await win.setMinSize(null);
  await win.setResizable(false);
  await win.setSize(new LogicalSize(460, 640));
  await win.center();
}

/** 工作台窗口：登录成功后微信式开新窗（#/welcome 酒店选择页）；fresh=显式登录，必经选择页 */
export async function openWorkbenchWindow(fresh = false) {
  const existing = await WebviewWindow.getByLabel("workbench");
  if (existing) {
    await existing.setFocus();
    return existing;
  }
  return new WebviewWindow("workbench", {
    title: "Device Scan",
    url: fresh ? "/#/welcome?fresh=1" : "/#/welcome",
    width: 1360,
    height: 860,
    minWidth: 1080,
    minHeight: 700,
    center: true,
    resizable: true,
    decorations: isMac,
    titleBarStyle: isMac ? "overlay" : "visible",
    hiddenTitle: true,
    // 红绿灯在 40px 自绘顶栏内垂直居中（按钮高 12px → y=14）
    trafficLightPosition: isMac ? new LogicalPosition(12, 14) : undefined,
  });
}

/** 登录窗口：退出登录后重建（label 复用 main） */
export async function openLoginWindow() {
  const existing = await WebviewWindow.getByLabel("main");
  if (existing) {
    await existing.setFocus();
    return existing;
  }
  return new WebviewWindow("main", {
    title: "Device Scan",
    url: "/#/login",
    width: 460,
    height: 640,
    center: true,
    resizable: false,
    decorations: isMac,
    titleBarStyle: isMac ? "overlay" : "visible",
    hiddenTitle: true,
  });
}

/**
 * 入住机管理台窗口：无边框 + 自绘顶栏（地球图标转系统浏览器），
 * 内容区由 BrowserView 创建子 webview 加载设备管理页。
 * label 按设备 host 去重：同一台设备复窗聚焦，不同设备各开一窗。
 */
export async function openAdminWindow(url: string, title: string) {
  let host = "device";
  try {
    host = new URL(url).host.replace(/[.:]/g, "-");
  } catch {
    /* 非法 URL 由后续加载失败兜底 */
  }
  const label = `admin-${host}`;
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.setFocus();
    return existing;
  }
  return new WebviewWindow(label, {
    title,
    url: `/#/browser?url=${encodeURIComponent(url)}&title=${encodeURIComponent(title)}`,
    width: 1200,
    height: 800,
    minWidth: 720,
    minHeight: 480,
    center: true,
    resizable: true,
    // macOS 用原生红绿灯（overlay 嵌入自绘顶栏，可拖动/关闭）；
    // 纯 decorations:false 在 mac 上无原生控件会导致窗口无法关闭
    decorations: isMac,
    titleBarStyle: isMac ? "overlay" : "visible",
    hiddenTitle: true,
    trafficLightPosition: isMac ? new LogicalPosition(12, 14) : undefined,
  });
}

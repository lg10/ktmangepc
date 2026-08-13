import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
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

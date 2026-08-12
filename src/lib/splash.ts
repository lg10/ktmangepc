/**
 * 原生闪屏控制：闪屏是 index.html 中独立于 #app 的 DOM 节点，
 * Vue 挂载后就地更新它（真实日志/进度），业务页就绪后淡出移除，全程零跳变。
 */
function el(id: string): HTMLElement | null {
  return document.getElementById(id);
}

export function splashTask(text: string) {
  const n = el("kt-splash-task");
  if (n) n.textContent = text;
}

export function splashLog(text: string) {
  const n = el("kt-splash-log");
  if (n) n.textContent = text;
}

export function splashProgress(percent: number) {
  const n = el("kt-splash-bar-fill");
  if (n) n.style.width = `${Math.min(Math.max(percent, 6), 100)}%`;
}

export function splashVersion(v: string) {
  const n = el("kt-splash-ver");
  if (n && v) n.textContent = `v${v}`;
}

/** 业务页就绪后淡出并移除闪屏节点 */
export function splashFinish() {
  const n = el("kt-splash");
  if (!n) return;
  n.style.transition = "opacity 0.28s ease";
  n.style.opacity = "0";
  setTimeout(() => n.remove(), 320);
}

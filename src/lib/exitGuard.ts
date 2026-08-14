// 退出 / 退出登录守卫（对应弹窗组件 ExitGuardDialog.vue）：
// DHCP / 中继运行中不允许直接退出，先停止服务恢复网卡，再执行后续动作。

import { ref } from "vue";
import { api } from "@/lib/api";

/** 等待弹窗是否打开 */
export const guardOpen = ref(false);
/** 恢复失败：弹窗展示错误并给出「重试恢复 / 直接退出」 */
export const guardFailed = ref(false);
export const guardError = ref("");

/** 恢复成功后要执行的后续动作（销毁窗口 / 退出登录流程） */
let pendingAfter: (() => void | Promise<void>) | null = null;

/** 依次停止内置 DHCP 与网络中继（各自尽力而为，失败汇总抛出） */
async function stopAll() {
  const errs: string[] = [];
  try {
    const d = await api.dhcpStatus();
    if (d.running) await api.stopDhcp();
  } catch (e) {
    errs.push(`DHCP: ${e}`);
  }
  try {
    const i = await api.inetShareStatus();
    if (i.running) await api.stopInetShare();
  } catch (e) {
    errs.push(`网络中继: ${e}`);
  }
  if (errs.length) throw new Error(errs.join("；"));
}

/** 恢复网络并执行后续动作；失败时保持弹窗供重试或强制退出 */
export async function restoreNetwork(after: () => void | Promise<void>) {
  pendingAfter = after;
  guardOpen.value = true;
  await tryStop();
}

/** （重试）执行停止流程 */
export async function tryStop() {
  guardFailed.value = false;
  guardError.value = "";
  try {
    await stopAll();
    guardOpen.value = false;
    const after = pendingAfter;
    pendingAfter = null;
    if (after) await after();
  } catch (e) {
    guardFailed.value = true;
    guardError.value = String(e);
  }
}

/** 强制退出：直接结束进程（特权助手若在运行，其 90 秒看门狗会兜底还原网络） */
export function forceQuit() {
  void api.quitNow();
}

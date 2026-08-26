<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, EVENTS } from "@/lib/api";
import { useUiStore } from "@/stores/ui";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { RefreshCw, PowerOff, Smartphone } from "lucide-vue-next";
import { Button } from "@/components/ui/button";
import AdbDevicePanel from "@/components/AdbDevicePanel.vue";

/**
 * ADB 终端弹窗：内置 adb（PATH 注入）的完整系统 shell 会话。
 * 关闭弹窗即销毁 shell 进程，下次打开重新 spawn（与后端 adb_shell 语义一致）。
 */
const uiStore = useUiStore();

const hostRef = ref<HTMLElement | null>(null);
const closed = ref(false);
const openError = ref("");
const panelOpen = ref(false);

let term: Terminal | null = null;
let fit: FitAddon | null = null;
let ro: ResizeObserver | null = null;
let unlistenData: UnlistenFn | null = null;
let unlistenClosed: UnlistenFn | null = null;

const encoder = new TextEncoder();

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function spawn() {
  closed.value = false;
  openError.value = "";
  try {
    await api.adbShellOpen();
  } catch (e) {
    openError.value = String(e);
  }
}

watch(
  () => uiStore.adbTerminalOpen,
  async (open) => {
    if (open) {
      await nextTick();
      if (!hostRef.value) return;
      term = new Terminal({
        fontSize: 13,
        fontFamily: "Menlo, Monaco, 'Courier New', monospace",
        cursorBlink: true,
        scrollback: 8000,
        theme: {
          background: "#1e1e2e",
          foreground: "#cdd6f4",
          cursor: "#89b4fa",
          selectionBackground: "rgba(137,180,250,0.25)",
        },
      });
      fit = new FitAddon();
      term.loadAddon(fit);
      term.open(hostRef.value);
      fit.fit();
      term.focus();
      term.onData((d) => {
        api.adbShellWrite(Array.from(encoder.encode(d))).catch(() => {});
      });

      unlistenData = await listen<{ id: string; data: string }>(
        EVENTS.ADB_DATA,
        (e) => {
          const bytes = b64ToBytes(e.payload.data);
          // shell 输出按 UTF-8 解码（adb/shell 现代环境默认 UTF-8）
          term?.write(new TextDecoder("utf-8", { fatal: false }).decode(bytes));
        }
      );
      unlistenClosed = await listen(EVENTS.ADB_CLOSED, () => {
        closed.value = true;
      });

      ro = new ResizeObserver(() => fit?.fit());
      ro.observe(hostRef.value);

      await spawn();
    } else {
      panelOpen.value = false;
      await teardown(true);
    }
  }
);

/** 清理终端资源；kill=true 时同时杀掉 shell 进程（关窗即销毁） */
async function teardown(kill: boolean) {
  unlistenData?.();
  unlistenClosed?.();
  unlistenData = unlistenClosed = null;
  ro?.disconnect();
  ro = null;
  term?.dispose();
  term = null;
  fit = null;
  closed.value = false;
  if (kill) await api.adbShellClose().catch(() => {});
}

/** shell 进程自行退出后的一键重开 */
async function reopen() {
  term?.clear();
  await spawn();
  closed.value = false;
}

onBeforeUnmount(() => {
  void teardown(true);
});
</script>

<template>
  <Dialog :open="uiStore.adbTerminalOpen" @update:open="uiStore.adbTerminalOpen = $event">
    <DialogContent
      class="max-w-6xl w-[80vw] h-[80vh] flex flex-col gap-0 p-0 overflow-hidden"
      :show-close="!panelOpen"
    >
      <div class="shrink-0 flex items-center px-6 pt-4 pb-2">
        <DialogTitle class="text-base">ADB 终端</DialogTitle>
        <span class="ml-2 text-xs text-muted-foreground">
          内置 adb 已注入 PATH，直接敲 adb 即可使用
        </span>
        <Button
          variant="ghost"
          size="sm"
          class="ml-auto h-7 gap-1.5 text-xs"
          @click="panelOpen = !panelOpen"
        >
          <Smartphone class="h-3.5 w-3.5" />
          已连接设备
        </Button>
      </div>

      <div class="flex-1 min-h-0 px-4 pb-4">
        <div class="relative h-full rounded-md overflow-hidden">
          <div ref="hostRef" class="absolute inset-0" />

          <!-- shell 进程退出覆盖层 -->
          <div
            v-if="closed"
            class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-[#1e1e2e]/95"
          >
            <PowerOff class="h-5 w-5 text-muted-foreground/70" />
            <p class="text-xs text-muted-foreground">Shell 会话已结束</p>
            <button
              class="flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs hover:bg-accent transition-colors"
              @click="reopen"
            >
              <RefreshCw class="h-3.5 w-3.5" />
              重新打开
            </button>
          </div>

          <!-- 启动失败提示 -->
          <div
            v-else-if="openError"
            class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-[#1e1e2e]/95"
          >
            <p class="text-xs text-destructive max-w-md text-center">{{ openError }}</p>
          </div>
        </div>
      </div>

      <AdbDevicePanel v-model:open="panelOpen" />
    </DialogContent>
  </Dialog>
</template>

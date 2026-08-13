<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, EVENTS } from "@/lib/api";
import { GbkDecoder, gbkEncode } from "@/lib/telnetCodec";
import { useTelnetStore } from "@/stores/telnet";
import { Loader2, RefreshCw, WifiOff } from "lucide-vue-next";

const props = defineProps<{ id: string; ip: string; visible: boolean }>();
const telnetStore = useTelnetStore();

const hostRef = ref<HTMLElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon | null = null;
let unlisten: UnlistenFn | null = null;
let ro: ResizeObserver | null = null;
// 设备中文输出为 GBK（原 TelnetDialog 用 ASCII 流，中文同样乱码），这里按 GBK 解码、ASCII 透传
const decoder = new GbkDecoder();

const tab = computed(() => telnetStore.tabs.find((t) => t.id === props.id));

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

onMounted(async () => {
  term = new Terminal({
    fontSize: 12,
    fontFamily: "Menlo, Monaco, 'Courier New', monospace",
    cursorBlink: true,
    scrollback: 5000,
    allowProposedApi: true,
    theme: {
      background: "#ffffff",
      foreground: "#1e293b",
      cursor: "#2563eb",
      selectionBackground: "rgba(37,99,235,0.18)",
    },
  });
  fit = new FitAddon();
  term.loadAddon(fit);
  term.open(hostRef.value!);
  fit.fit();
  term.writeln(`\x1b[90m── Telnet ${props.id} (${props.ip}) ──\x1b[0m`);
  term.onData((d) => {
    // 输入侧同样按 GBK 编码下发，保证中文命令设备可识别
    api.telnetWrite(props.id, Array.from(gbkEncode(d))).catch(() => {});
  });

  unlisten = await listen<{ id: string; data: string }>(EVENTS.TELNET_DATA, (e) => {
    if (e.payload.id !== props.id) return;
    const text = decoder.push(b64ToBytes(e.payload.data));
    if (text) term?.write(text);
  });

  ro = new ResizeObserver(() => {
    if (props.visible) fit?.fit();
  });
  ro.observe(hostRef.value!);
});

watch(
  () => props.visible,
  (v) => {
    if (v) {
      fit?.fit();
      term?.focus();
    }
  }
);

onBeforeUnmount(() => {
  unlisten?.();
  ro?.disconnect();
  term?.dispose();
});

function reconnect() {
  telnetStore.reconnect(props.id);
  decoder.reset();
  term?.clear();
}
</script>

<template>
  <div class="h-full relative">
    <div ref="hostRef" class="absolute inset-0 p-1" :class="visible ? '' : 'pointer-events-none'" />

    <!-- 连接中 / 已断开覆盖层 -->
    <div
      v-if="tab && tab.status !== 'connected'"
      class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-background/90"
    >
      <template v-if="tab.status === 'connecting'">
        <Loader2 class="h-5 w-5 animate-spin text-primary" />
        <p class="text-xs text-muted-foreground">正在连接 {{ ip }}:{{ tab.port }}…</p>
      </template>
      <template v-else>
        <WifiOff class="h-5 w-5 text-muted-foreground/60" />
        <p class="text-xs text-muted-foreground">{{ tab.reason || "连接已断开" }}</p>
        <button
          class="flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs hover:bg-accent transition-colors"
          @click="reconnect"
        >
          <RefreshCw class="h-3.5 w-3.5" />
          重新连接
        </button>
      </template>
    </div>
  </div>
</template>

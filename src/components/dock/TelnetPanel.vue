<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, EVENTS } from "@/lib/api";
import { GbkDecoder, gbkEncode } from "@/lib/telnetCodec";
import { parseCompletion } from "@/lib/telnetCompletion";
import { LogHighlighter } from "@/lib/logHighlight";
import { COMMAND_FORMATTERS, type CommandFormatter } from "@/lib/commandFormatters";
import { useTelnetStore } from "@/stores/telnet";
import { useUiStore } from "@/stores/ui";
import { Loader2, RefreshCw, WifiOff } from "lucide-vue-next";
import TelnetInputBar from "@/components/dock/TelnetInputBar.vue";
import QuickCommandBar from "@/components/dock/QuickCommandBar.vue";

const props = defineProps<{ id: string; ip: string; visible: boolean }>();
const telnetStore = useTelnetStore();
const uiStore = useUiStore();

const hostRef = ref<HTMLElement | null>(null);
const inputBar = ref<InstanceType<typeof TelnetInputBar> | null>(null);
const completing = ref(false);
// 设备中文输出为 GBK（原 TelnetDialog 用 ASCII 流，中文同样乱码），这里按 GBK 解码、ASCII 透传
const decoder = new GbkDecoder();

let term: Terminal | null = null;
let fit: FitAddon | null = null;
let unlisten: UnlistenFn | null = null;
let ro: ResizeObserver | null = null;

const tab = computed(() => telnetStore.tabs.find((t) => t.id === props.id));
const connected = computed(() => tab.value?.status === "connected");
const isRaw = computed(() => uiStore.terminalInputMode === "raw");

const highlighter = new LogHighlighter();
let highlightTimer: number | null = null;

/** 日志统一入口：按行高亮，半截行最多等 80ms 再上屏 */
function writeToTerm(text: string) {
  if (!text) return;
  const ready = highlighter.push(text);
  if (ready) term?.write(ready);
  if (highlighter.pending) {
    if (highlightTimer !== null) window.clearTimeout(highlightTimer);
    highlightTimer = window.setTimeout(flushHighlight, 80);
  } else {
    cancelHighlightFlush();
  }
}

function flushHighlight() {
  if (highlightTimer !== null) {
    window.clearTimeout(highlightTimer);
    highlightTimer = null;
  }
  const rest = highlighter.flush();
  if (rest) term?.write(rest);
}

function cancelHighlightFlush() {
  if (highlightTimer !== null) {
    window.clearTimeout(highlightTimer);
    highlightTimer = null;
  }
}

// Tab 查询会临时截留设备输出用于解析候选，日志随后补写回终端。
// 真机响应在 0.1–2s 间波动，因此基础窗口给到 3s，识别到补全特征后 350ms 内提前收尾。
const CAPTURE_MAX_MS = 3000;
const CAPTURE_SETTLE_MS = 350;
// 候选签名（TAB + 候选 + NUL）或提示符重绘出现，说明补全输出已经开始/结束
const COMPLETION_HINT = /~ # |\t[^\s\0]+\0/;

let capture = "";
let captureSent = "";
let captureStarted = 0;
let captureTimer: number | null = null;

/** 唯一补全只回显补全后的整词，没有候选列表和提示符，需单独识别 */
function hasUniqueEcho(raw: string, sent: string): boolean {
  if (!sent) return false;
  const esc = sent.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(?:^|\\n)${esc}\\S+\\s*(?=\\r?\\n|$)`).test(raw);
}

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function writeToDevice(text: string) {
  api.telnetWrite(props.id, Array.from(gbkEncode(text))).catch(() => {});
}

function requestCompletion(prefix: string) {
  if (completing.value || !connected.value) return;
  finishCommandCapture();
  flushHighlight();
  completing.value = true;
  capture = "";
  captureSent = prefix;
  captureStarted = Date.now();
  writeToDevice(`${prefix}\t`);
  scheduleCaptureEnd(CAPTURE_MAX_MS);
}

function scheduleCaptureEnd(ms: number) {
  if (captureTimer !== null) window.clearTimeout(captureTimer);
  const remain = CAPTURE_MAX_MS - (Date.now() - captureStarted);
  captureTimer = window.setTimeout(finishCompletion, Math.max(0, Math.min(ms, remain)));
}

function finishCompletion() {
  captureTimer = null;
  if (!completing.value) return;
  completing.value = false;
  const raw = capture;
  const sent = captureSent;
  capture = "";
  captureSent = "";

  const result = parseCompletion(raw, sent);
  if (result.leftover) writeToTerm(result.leftover);
  // 清掉设备行缓冲里查询用的半截命令，否则下次回车会拼成 debug-debug-ethernet
  const cleanup = "\x7f".repeat(result.remoteLine.length);
  if (cleanup) writeToDevice(cleanup);
  inputBar.value?.applyCompletion(sent, result);
}

function cancelCapture() {
  if (captureTimer !== null) window.clearTimeout(captureTimer);
  captureTimer = null;
  completing.value = false;
  capture = "";
  captureSent = "";
}

// 已注册格式化器的命令：发送后截留输出，等 isComplete 或超时后再统一上屏
const COMMAND_CAPTURE_MAX_MS = 5000;
const COMMAND_CAPTURE_SETTLE_MS = 150;

let commandCapture: {
  command: string;
  formatter: CommandFormatter;
  raw: string;
  startedAt: number;
  timer: number | null;
} | null = null;

function startCommandCapture(command: string, formatter: CommandFormatter) {
  if (commandCapture) finishCommandCapture();
  commandCapture = { command, formatter, raw: "", startedAt: Date.now(), timer: null };
  scheduleCommandFinish(COMMAND_CAPTURE_MAX_MS);
}

function scheduleCommandFinish(ms: number) {
  if (!commandCapture) return;
  if (commandCapture.timer !== null) window.clearTimeout(commandCapture.timer);
  const remain = COMMAND_CAPTURE_MAX_MS - (Date.now() - commandCapture.startedAt);
  commandCapture.timer = window.setTimeout(finishCommandCapture, Math.max(0, Math.min(ms, remain)));
}

function finishCommandCapture() {
  if (!commandCapture) return;
  if (commandCapture.timer !== null) window.clearTimeout(commandCapture.timer);
  const cap = commandCapture;
  commandCapture = null;
  const formatted = cap.formatter.format(cap.raw, cap.command);
  if (formatted) writeToTerm(`${formatted}\r\n`);
  else if (cap.raw) writeToTerm(cap.raw);
}

function cancelCommandCapture() {
  if (commandCapture?.timer != null) window.clearTimeout(commandCapture.timer);
  commandCapture = null;
}

function sendLine(line: string) {
  const command = line.trim();
  const formatter = COMMAND_FORMATTERS[command];
  if (formatter && connected.value) startCommandCapture(command, formatter);
  else if (commandCapture) finishCommandCapture();
  writeToDevice(`${line}\r`);
}

function interrupt() {
  finishCommandCapture();
  writeToDevice("\x03");
}

function onTerminalClick() {
  // 拖选日志时不抢焦点，保留 Cmd+C 复制
  if (term?.hasSelection()) return;
  if (isRaw.value) term?.focus();
  else inputBar.value?.focus();
}

onMounted(async () => {
  term = new Terminal({
    fontSize: 12,
    fontFamily: "Menlo, Monaco, 'Courier New', monospace",
    cursorBlink: true,
    scrollback: 5000,
    // 默认输入走底部固定行，终端区只负责渲染日志；切到直输模式时再打开 stdin
    disableStdin: uiStore.terminalInputMode !== "raw",
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

  unlisten = await listen<{ id: string; data: string }>(EVENTS.TELNET_DATA, (e) => {
    if (e.payload.id !== props.id) return;
    const text = decoder.push(b64ToBytes(e.payload.data));
    if (!text) return;
    if (completing.value) {
      capture += text;
      if (COMPLETION_HINT.test(capture) || hasUniqueEcho(capture, captureSent)) {
        scheduleCaptureEnd(CAPTURE_SETTLE_MS);
      }
      return;
    }
    if (commandCapture) {
      commandCapture.raw += text;
      if (commandCapture.formatter.isComplete(commandCapture.raw)) {
        scheduleCommandFinish(COMMAND_CAPTURE_SETTLE_MS);
      }
      return;
    }
    writeToTerm(text);
  });

  // 直输模式下把键盘输入原样发给设备（Tab 也由设备自己补全）
  term.onData((data) => {
    if (uiStore.terminalInputMode === "raw") writeToDevice(data);
  });

  ro = new ResizeObserver(() => {
    if (props.visible) fit?.fit();
  });
  ro.observe(hostRef.value!);

  if (props.visible) focusInputTarget();
});

watch(
  () => props.visible,
  (v) => {
    if (v) {
      fit?.fit();
      focusInputTarget();
    }
  }
);

watch(
  () => uiStore.terminalInputMode,
  (mode) => {
    if (!term) return;
    if (completing.value) finishCompletion();
    finishCommandCapture();
    term.options.disableStdin = mode !== "raw";
    if (props.visible) focusInputTarget();
  }
);

watch(
  () => tab.value?.status,
  (s) => {
    if (s !== "connected") {
      cancelCapture();
      finishCommandCapture();
      inputBar.value?.reset();
    } else if (props.visible) {
      focusInputTarget();
    }
  }
);

function focusInputTarget() {
  if (isRaw.value) term?.focus();
  else inputBar.value?.focus();
}

onBeforeUnmount(() => {
  cancelCapture();
  cancelCommandCapture();
  cancelHighlightFlush();
  highlighter.reset();
  unlisten?.();
  ro?.disconnect();
  term?.dispose();
});

function reconnect() {
  cancelCapture();
  cancelCommandCapture();
  cancelHighlightFlush();
  highlighter.reset();
  inputBar.value?.reset();
  telnetStore.reconnect(props.id);
  decoder.reset();
  term?.clear();
}
</script>

<template>
  <div class="h-full flex flex-col">
    <div class="relative min-h-0 flex-1">
      <div
        ref="hostRef"
        class="absolute inset-0 p-1"
        :class="visible ? '' : 'pointer-events-none'"
        @click="onTerminalClick"
      />

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

    <TelnetInputBar
      v-show="!isRaw"
      ref="inputBar"
      :disabled="!connected"
      :busy="completing"
      @send="sendLine"
      @request-completion="requestCompletion"
      @interrupt="interrupt"
    />
    <QuickCommandBar :disabled="!connected" @send="sendLine" />
  </div>
</template>

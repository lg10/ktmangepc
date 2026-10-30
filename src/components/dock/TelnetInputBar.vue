<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { Loader2 } from "lucide-vue-next";
import type { CompletionResult } from "@/lib/telnetCompletion";

const props = defineProps<{ disabled?: boolean; busy?: boolean }>();
const emit = defineEmits<{
  (e: "send", line: string): void;
  (e: "request-completion", prefix: string): void;
  (e: "interrupt"): void;
}>();

const text = ref("");
const candidates = ref<string[]>([]);
const active = ref(-1);
const listPrefix = ref("");
const history = ref<string[]>([]);
const historyIndex = ref<number | null>(null);
const draft = ref("");
const inputRef = ref<HTMLInputElement | null>(null);

function focus() {
  nextTick(() => inputRef.value?.focus());
}

function closeList() {
  candidates.value = [];
  active.value = -1;
  listPrefix.value = "";
}

function reset() {
  text.value = "";
  closeList();
  historyIndex.value = null;
  draft.value = "";
}

/** 查询结果回填：只有输入仍是发起查询时的内容才采纳，避免覆盖查询期间的新输入 */
function applyCompletion(sentLine: string, result: CompletionResult) {
  if (props.disabled || text.value !== sentLine || result.candidates.length === 0) return;
  const tokenStart = sentLine.lastIndexOf(" ") + 1;
  const sentToken = sentLine.slice(tokenStart);
  if (!result.candidates.every((c) => c.startsWith(sentToken))) return;
  text.value = result.lcp;
  if (result.candidates.length === 1) {
    closeList();
  } else {
    listPrefix.value = sentLine.slice(0, tokenStart);
    candidates.value = [...result.candidates];
    active.value = -1;
  }
  focus();
}

function submit() {
  if (props.disabled || props.busy) return;
  const line = text.value.trimEnd();
  closeList();
  historyIndex.value = null;
  text.value = "";
  if (!line) return;
  if (history.value[history.value.length - 1] !== line) history.value.push(line);
  emit("send", line);
}

function pick(i: number) {
  const c = candidates.value[i];
  if (!c) return;
  active.value = i;
  text.value = listPrefix.value + c;
}

function cycleList(step: 1 | -1) {
  const n = candidates.value.length;
  pick(active.value < 0 ? (step > 0 ? 0 : n - 1) : (active.value + step + n) % n);
}

function onTab(shift: boolean) {
  if (props.disabled || props.busy) return;
  if (candidates.value.length) cycleList(shift ? -1 : 1);
  else if (!shift) emit("request-completion", text.value);
}

function onUp() {
  if (candidates.value.length) {
    cycleList(-1);
    return;
  }
  if (!history.value.length) return;
  if (historyIndex.value === null) {
    draft.value = text.value;
    historyIndex.value = history.value.length - 1;
  } else if (historyIndex.value > 0) {
    historyIndex.value--;
  }
  text.value = history.value[historyIndex.value];
}

function onDown() {
  if (candidates.value.length) {
    cycleList(1);
    return;
  }
  if (historyIndex.value === null) return;
  if (historyIndex.value < history.value.length - 1) {
    historyIndex.value++;
    text.value = history.value[historyIndex.value];
  } else {
    historyIndex.value = null;
    text.value = draft.value;
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.ctrlKey && !e.metaKey && (e.key === "c" || e.key === "C")) {
    e.preventDefault();
    closeList();
    text.value = "";
    historyIndex.value = null;
    emit("interrupt");
    return;
  }
  switch (e.key) {
    case "Enter":
      e.preventDefault();
      submit();
      break;
    case "Tab":
      e.preventDefault();
      onTab(e.shiftKey);
      break;
    case "Escape":
      if (candidates.value.length) {
        e.preventDefault();
        closeList();
      }
      break;
    case "ArrowUp":
      e.preventDefault();
      onUp();
      break;
    case "ArrowDown":
      e.preventDefault();
      onDown();
      break;
  }
}

function onInput() {
  closeList();
  historyIndex.value = null;
}

function choose(i: number) {
  pick(i);
  closeList();
  focus();
}

watch(
  () => props.disabled,
  (v) => {
    if (v) closeList();
  }
);

defineExpose({ applyCompletion, focus, reset });
</script>

<template>
  <div class="relative z-20 shrink-0 border-t bg-background">
    <!-- 候选浮层：贴在输入行上方，不随日志滚动 -->
    <div v-if="candidates.length" class="absolute inset-x-0 bottom-full border-x border-t bg-popover shadow-lg">
      <ul class="max-h-56 overflow-auto py-1">
        <li
          v-for="(c, i) in candidates"
          :key="`${i}-${c}`"
          class="cursor-pointer px-3 py-1.5 font-mono text-xs"
          :class="i === active ? 'bg-accent text-accent-foreground' : 'text-foreground/80'"
          @mouseenter="active = i"
          @mousedown.prevent="choose(i)"
        >
          {{ c }}
        </li>
      </ul>
      <div class="flex items-center justify-between border-t px-3 py-1 text-[10px] text-muted-foreground">
        <span>Tab 循环 · Shift+Tab 反向</span>
        <span>Enter 发送 · Esc 关闭</span>
      </div>
    </div>

    <div class="flex h-9 items-center gap-2 px-2.5">
      <span class="shrink-0 select-none font-mono text-xs text-muted-foreground">~ #</span>
      <input
        ref="inputRef"
        v-model="text"
        :disabled="disabled"
        class="min-w-0 flex-1 bg-transparent font-mono text-xs text-foreground outline-none placeholder:text-muted-foreground/70 disabled:cursor-not-allowed"
        placeholder="输入命令，Enter 发送，Tab 补全"
        autocomplete="off"
        autocapitalize="off"
        autocorrect="off"
        spellcheck="false"
        @keydown="onKeydown"
        @input="onInput"
      />
      <template v-if="busy">
        <Loader2 class="h-3.5 w-3.5 shrink-0 animate-spin text-muted-foreground" />
        <span class="hidden shrink-0 text-[10px] text-muted-foreground/70 sm:block">
          补全查询中…
        </span>
      </template>
      <span v-else class="hidden shrink-0 text-[10px] text-muted-foreground/70 sm:block">
        Tab 补全 · ↑↓ 历史
      </span>
    </div>
  </div>
</template>

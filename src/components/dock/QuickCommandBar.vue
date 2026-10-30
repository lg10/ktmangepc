<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { Keyboard, Plus, TextCursorInput, X } from "lucide-vue-next";
import { MAX_QUICK_COMMANDS, useQuickCommandStore } from "@/stores/quickCommands";
import { useUiStore } from "@/stores/ui";

const props = defineProps<{ disabled?: boolean }>();
const emit = defineEmits<{ (e: "send", command: string): void }>();

const quickStore = useQuickCommandStore();
const uiStore = useUiStore();

const formOpen = ref(false);
const name = ref("");
const command = ref("");
const nameRef = ref<HTMLInputElement | null>(null);

const isRaw = computed(() => uiStore.terminalInputMode === "raw");
const canSave = computed(() => !!name.value.trim() && !!command.value.trim());

function openForm() {
  formOpen.value = true;
  name.value = "";
  command.value = "";
  nextTick(() => nameRef.value?.focus());
}

function closeForm() {
  formOpen.value = false;
}

function save() {
  if (!canSave.value) return;
  if (quickStore.add(name.value, command.value)) closeForm();
}

function toggleMode() {
  uiStore.setTerminalInputMode(isRaw.value ? "bar" : "raw");
}

function send(itemCommand: string) {
  if (props.disabled) return;
  emit("send", itemCommand);
}
</script>

<template>
  <div class="relative z-20 flex h-8 shrink-0 items-center border-t bg-muted/20">
    <div class="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto px-2">
      <div
        v-for="item in quickStore.items"
        :key="item.id"
        class="group flex h-6 shrink-0 items-center rounded-full border border-border/70 bg-background pr-1 transition-colors hover:border-primary/40 hover:bg-accent"
        :class="disabled && 'opacity-50'"
      >
        <button
          type="button"
          class="h-full pl-2.5 pr-1 font-mono text-[11px] text-foreground/80 disabled:cursor-not-allowed"
          :disabled="disabled"
          :title="`${item.command}（点击发送）`"
          @click="send(item.command)"
        >
          {{ item.name }}
        </button>
        <button
          type="button"
          class="rounded-full p-0.5 text-muted-foreground/40 opacity-0 transition-opacity hover:bg-destructive/10 hover:text-destructive group-hover:opacity-100"
          title="删除快捷命令"
          @click="quickStore.remove(item.id)"
        >
          <X class="h-3 w-3" />
        </button>
      </div>

      <button
        v-if="quickStore.items.length < MAX_QUICK_COMMANDS"
        type="button"
        class="flex h-6 shrink-0 items-center gap-1 rounded-full border border-dashed px-2 text-[11px] text-muted-foreground transition-colors hover:border-primary/40 hover:text-foreground"
        title="添加快捷命令（最多 6 条）"
        @click="openForm"
      >
        <Plus class="h-3 w-3" />
        <span v-if="quickStore.items.length === 0">添加快捷命令</span>
      </button>
      <span v-else class="shrink-0 text-[10px] text-muted-foreground/50">已满 6 条</span>
    </div>

    <div class="flex shrink-0 items-center border-l px-1.5">
      <button
        type="button"
        class="flex h-6 items-center gap-1.5 rounded-md px-1.5 text-[10px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        :title="
          isRaw
            ? '当前：终端直输，点击切回底部固定输入行'
            : '当前：底部固定输入行，点击切到终端直输'
        "
        @click="toggleMode"
      >
        <component :is="isRaw ? Keyboard : TextCursorInput" class="h-3.5 w-3.5" />
        <span class="hidden sm:inline">{{ isRaw ? "终端直输" : "固定输入行" }}</span>
      </button>
    </div>

    <!-- 添加表单浮层：贴在这一行上方 -->
    <div v-if="formOpen" class="fixed inset-0 z-20" @mousedown="closeForm" />
    <div
      v-if="formOpen"
      class="absolute bottom-full left-2 z-30 mb-1 w-72 rounded-md border bg-popover p-2 shadow-lg"
    >
      <div class="mb-2 flex items-center justify-between">
        <span class="text-[11px] font-medium text-foreground/80">添加快捷命令</span>
        <span class="text-[10px] text-muted-foreground/60">{{ quickStore.items.length }}/{{ MAX_QUICK_COMMANDS }}</span>
      </div>
      <input
        ref="nameRef"
        v-model="name"
        maxlength="12"
        placeholder="名称（显示在按钮上）"
        class="mb-1.5 w-full rounded border bg-background px-2 py-1 text-xs outline-none placeholder:text-muted-foreground/60 focus:border-primary/60"
        autocomplete="off"
        spellcheck="false"
        @keydown.enter.prevent="save"
        @keydown.esc.prevent="closeForm"
      />
      <input
        v-model="command"
        maxlength="200"
        placeholder="命令（点击按钮后直接发送）"
        class="mb-2 w-full rounded border bg-background px-2 py-1 font-mono text-xs outline-none placeholder:text-muted-foreground/60 focus:border-primary/60"
        autocomplete="off"
        autocapitalize="off"
        autocorrect="off"
        spellcheck="false"
        @keydown.enter.prevent="save"
        @keydown.esc.prevent="closeForm"
      />
      <div class="flex justify-end gap-1.5">
        <button
          type="button"
          class="rounded border px-2 py-1 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
          @click="closeForm"
        >
          取消
        </button>
        <button
          type="button"
          class="rounded bg-primary px-2 py-1 text-[11px] text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="!canSave"
          @click="save"
        >
          保存
        </button>
      </div>
    </div>
  </div>
</template>

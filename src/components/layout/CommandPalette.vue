<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { api } from "@/lib/api";
import { useUiStore } from "@/stores/ui";
import { useDeviceStore } from "@/stores/device";
import { useToast } from "@/components/ui/toast/use-toast";
import {
  Rocket,
  FolderArchive,
  Settings,
  Square,
  PanelBottom,
  PanelLeft,
  Trash2,
  Search,
} from "lucide-vue-next";

const router = useRouter();
const uiStore = useUiStore();
const deviceStore = useDeviceStore();
const { toast } = useToast();

const query = ref("");
const cursor = ref(0);
const inputRef = ref<HTMLInputElement | null>(null);

interface Cmd {
  label: string;
  hint?: string;
  icon: unknown;
  run: () => void | Promise<void>;
}

const commands = computed<Cmd[]>(() => [
  { label: "前往：工作模式", icon: Rocket, run: () => router.push({ name: "launch" }) },
  { label: "打开：文件库", icon: FolderArchive, run: () => uiStore.openFiles() },
  { label: "打开：设置", icon: Settings, run: () => uiStore.openSettings() },
  {
    label: deviceStore.status.running ? "停止 UDP 服务" : "启动 UDP 服务",
    icon: Square,
    run: async () => {
      if (deviceStore.status.running) {
        await api.stopUdpServer();
        toast({ title: "服务已停止" });
      } else {
        router.push({ name: "launch" });
      }
    },
  },
  {
    label: uiStore.dockOpen ? "收起底部面板" : "展开底部面板",
    hint: "任务 / 日志 / 终端",
    icon: PanelBottom,
    run: () => uiStore.toggleDock(),
  },
  {
    label: uiStore.sidebarCollapsed ? "展开侧栏" : "收起侧栏",
    icon: PanelLeft,
    run: () => uiStore.toggleSidebar(),
  },
  {
    label: "清空设备列表",
    icon: Trash2,
    run: async () => {
      await api.clearDevices();
      deviceStore.reset();
      toast({ title: "设备列表已清空" });
    },
  },
]);

const filtered = computed(() => {
  const kw = query.value.trim().toLowerCase();
  if (!kw) return commands.value;
  return commands.value.filter((c) => c.label.toLowerCase().includes(kw));
});

watch(
  () => uiStore.paletteOpen,
  async (open) => {
    if (open) {
      query.value = "";
      cursor.value = 0;
      await nextTick();
      inputRef.value?.focus();
    }
  }
);

function close() {
  uiStore.paletteOpen = false;
}

async function runCmd(cmd: Cmd) {
  close();
  await cmd.run();
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "ArrowDown") {
    e.preventDefault();
    cursor.value = Math.min(cursor.value + 1, filtered.value.length - 1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    cursor.value = Math.max(cursor.value - 1, 0);
  } else if (e.key === "Enter") {
    e.preventDefault();
    const cmd = filtered.value[cursor.value];
    if (cmd) runCmd(cmd);
  } else if (e.key === "Escape") {
    close();
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="uiStore.paletteOpen"
      class="fixed inset-0 z-50 bg-black/25 flex items-start justify-center pt-[12vh]"
      @mousedown.self="close"
    >
      <div class="w-[520px] max-w-[90vw] rounded-xl border bg-popover shadow-xl overflow-hidden">
        <div class="flex items-center gap-2 border-b px-3">
          <Search class="h-4 w-4 text-muted-foreground shrink-0" />
          <input
            ref="inputRef"
            v-model="query"
            class="w-full h-10 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            placeholder="输入命令或页面名称…"
            @keydown="onKeydown"
          />
          <kbd class="shrink-0 rounded border bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">ESC</kbd>
        </div>
        <ul class="max-h-72 overflow-auto py-1.5">
          <li
            v-for="(cmd, i) in filtered"
            :key="cmd.label"
            class="flex items-center gap-2.5 px-3 py-2 text-sm cursor-pointer"
            :class="i === cursor ? 'bg-accent text-accent-foreground' : 'text-foreground/80'"
            @mouseenter="cursor = i"
            @click="runCmd(cmd)"
          >
            <component :is="cmd.icon" class="h-4 w-4 text-muted-foreground shrink-0" />
            <span>{{ cmd.label }}</span>
            <span v-if="cmd.hint" class="ml-auto text-[11px] text-muted-foreground">{{ cmd.hint }}</span>
          </li>
          <li v-if="filtered.length === 0" class="px-3 py-6 text-center text-sm text-muted-foreground">
            没有匹配的命令
          </li>
        </ul>
      </div>
    </div>
  </Teleport>
</template>

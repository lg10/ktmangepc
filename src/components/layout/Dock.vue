<script setup lang="ts">
import { computed, ref } from "vue";
import { useUiStore } from "@/stores/ui";
import { useTelnetStore } from "@/stores/telnet";
import { useTaskStore } from "@/stores/task";
import TaskPanel from "@/components/dock/TaskPanel.vue";
import LogPanel from "@/components/dock/LogPanel.vue";
import TelnetPanel from "@/components/dock/TelnetPanel.vue";
import { ListChecks, ScrollText, Terminal, X, ChevronDown } from "lucide-vue-next";

const uiStore = useUiStore();
const telnetStore = useTelnetStore();
const taskStore = useTaskStore();

const active = computed(() => uiStore.dockTab);
const dragging = ref(false);

function startResize(e: MouseEvent) {
  dragging.value = true;
  const startY = e.clientY;
  const startH = uiStore.dockHeight;
  const move = (ev: MouseEvent) => {
    uiStore.setDockHeight(startH + (startY - ev.clientY));
  };
  const up = () => {
    dragging.value = false;
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

async function closeTelnet(id: string) {
  const wasActive = active.value === `telnet:${id}`;
  await telnetStore.close(id);
  if (wasActive) {
    const next = telnetStore.tabs[0];
    uiStore.setDockTab(next ? `telnet:${next.id}` : "tasks");
  }
}
</script>

<template>
  <section
    class="shrink-0 border-t bg-background flex flex-col"
    :style="{ height: uiStore.dockHeight + 'px' }"
  >
    <!-- 拖拽调整高度 -->
    <div
      class="h-1 -mt-0.5 cursor-row-resize z-20 hover:bg-primary/40"
      :class="dragging && 'bg-primary/60'"
      @mousedown.prevent="startResize"
    />

    <!-- 标签栏 -->
    <div class="h-8 shrink-0 flex items-center gap-1 px-2 border-b bg-muted/30 select-none overflow-x-auto">
      <button
        class="dock-tab"
        :class="active === 'tasks' && 'dock-tab-active'"
        @click="uiStore.setDockTab('tasks')"
      >
        <ListChecks class="h-3.5 w-3.5" />
        任务
        <span
          v-if="taskStore.activeCount"
          class="rounded-full bg-primary/15 text-primary text-[10px] px-1.5 leading-4"
        >
          {{ taskStore.activeCount }}
        </span>
      </button>
      <button
        class="dock-tab"
        :class="active === 'logs' && 'dock-tab-active'"
        @click="uiStore.setDockTab('logs')"
      >
        <ScrollText class="h-3.5 w-3.5" />
        日志
      </button>

      <template v-for="t in telnetStore.tabs" :key="t.id">
        <button
          class="dock-tab group"
          :class="active === `telnet:${t.id}` && 'dock-tab-active'"
          :title="`${t.id} · ${t.ip}:${t.port}`"
          @click="uiStore.setDockTab(`telnet:${t.id}`)"
        >
          <Terminal class="h-3.5 w-3.5" />
          <span class="font-mono max-w-28 truncate">{{ t.id }}</span>
          <span
            v-if="t.status === 'closed'"
            class="h-1.5 w-1.5 rounded-full bg-destructive/70"
            title="已断开"
          />
          <span
            class="rounded hover:bg-accent p-0.5 opacity-0 group-hover:opacity-100"
            title="关闭终端"
            @click.stop="closeTelnet(t.id)"
          >
            <X class="h-3 w-3" />
          </span>
        </button>
      </template>

      <button
        class="ml-auto text-muted-foreground hover:text-foreground transition-colors p-1"
        title="收起面板"
        @click="uiStore.toggleDock()"
      >
        <ChevronDown class="h-4 w-4" />
      </button>
    </div>

    <!-- 面板内容：全部挂载、v-show 切换（保留 xterm 缓冲） -->
    <div class="flex-1 min-h-0 relative">
      <div class="absolute inset-0" :class="active === 'tasks' ? '' : 'hidden'">
        <TaskPanel />
      </div>
      <div class="absolute inset-0" :class="active === 'logs' ? '' : 'hidden'">
        <LogPanel />
      </div>
      <div
        v-for="t in telnetStore.tabs"
        :key="t.id"
        class="absolute inset-0"
        :class="active === `telnet:${t.id}` ? '' : 'hidden' "
      >
        <TelnetPanel :id="t.id" :ip="t.ip" :visible="active === `telnet:${t.id}`" />
      </div>
    </div>
  </section>
</template>

<style scoped>
.dock-tab {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.25rem 0.625rem;
  border-radius: 0.375rem;
  font-size: 12px;
  color: var(--muted-foreground);
  white-space: nowrap;
  transition:
    color 0.15s,
    background-color 0.15s;
}
.dock-tab:hover {
  color: var(--foreground);
  background: var(--accent);
}
.dock-tab-active {
  color: var(--foreground);
  background: var(--accent);
  font-weight: 500;
}
</style>

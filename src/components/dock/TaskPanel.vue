<script setup lang="ts">
import { computed } from "vue";
import { useTaskStore } from "@/stores/task";
import { useToast } from "@/components/ui/toast/use-toast";
import { Badge } from "@/components/ui/badge";
import { Cpu, FileCog, X, Inbox, Eraser } from "lucide-vue-next";

const taskStore = useTaskStore();
const { toast } = useToast();

/** 已完成记录数（完成 / 超时 / 重复），仅这部分可清除 */
const finishedCount = computed(() => taskStore.tasks.filter((t) => t.state > 1).length);

function stateBadge(state: number) {
  if (state <= 1) return { label: state === 0 ? "等待" : "进行", variant: "secondary" as const };
  if (state === 10) return { label: "完成", variant: "success" as const };
  if (state === 999) return { label: "超时", variant: "warning" as const };
  return { label: "重复", variant: "outline" as const };
}

function fmtTime(ms: number) {
  if (!ms) return "—";
  return new Date(ms).toLocaleTimeString("zh-CN", { hour12: false });
}

async function cancel(equipId: string, kind: "firmware" | "config") {
  try {
    await taskStore.cancel(equipId, kind);
  } catch (e) {
    toast({ title: "取消失败", description: String(e), variant: "destructive" });
  }
}

async function clearFinished() {
  try {
    await taskStore.clearFinished();
  } catch (e) {
    toast({ title: "清除失败", description: String(e), variant: "destructive" });
  }
}
</script>

<template>
  <div class="h-full overflow-auto relative">
    <button
      v-if="finishedCount"
      class="absolute right-2 top-1.5 z-20 flex items-center gap-1 rounded border bg-background/90 px-1.5 py-0.5 text-[10px] text-muted-foreground hover:text-foreground"
      title="清除已完成 / 超时 / 重复记录（进行中任务不受影响）"
      @click="clearFinished"
    >
      <Eraser class="h-3 w-3" />
      清空
    </button>
    <table v-if="taskStore.tasks.length" class="w-full text-xs border-collapse">
      <thead class="sticky top-0 bg-muted/70 backdrop-blur z-10">
        <tr class="text-left text-muted-foreground">
          <th class="py-1.5 px-3 font-medium w-16">类型</th>
          <th class="py-1.5 px-3 font-medium w-40">设备</th>
          <th class="py-1.5 px-3 font-medium">文件</th>
          <th class="py-1.5 px-3 font-medium w-56">进度</th>
          <th class="py-1.5 px-3 font-medium w-20">状态</th>
          <th class="py-1.5 px-3 font-medium w-24 hidden min-[1000px]:table-cell">开始</th>
          <th class="py-1.5 px-3 font-medium w-24 hidden min-[1000px]:table-cell">结束</th>
          <th class="py-1.5 px-3 font-medium w-12"></th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="t in taskStore.tasks"
          :key="`${t.kind}-${t.equipId}-${t.startTime}`"
          class="border-b border-border/50"
        >
          <td class="py-1.5 px-3">
            <span class="flex items-center gap-1 text-muted-foreground">
              <Cpu v-if="t.kind === 'firmware'" class="h-3.5 w-3.5" />
              <FileCog v-else class="h-3.5 w-3.5" />
              {{ t.kind === "firmware" ? "固件" : "配置" }}
            </span>
          </td>
          <td class="py-1.5 px-3 font-mono">{{ t.equipId }}</td>
          <td class="py-1.5 px-3 truncate max-w-0">{{ t.fileName }}</td>
          <td class="py-1.5 px-3">
            <div class="flex items-center gap-2">
              <div class="flex-1 h-1.5 rounded-full bg-muted overflow-hidden min-w-16">
                <div
                  class="h-full rounded-full transition-all"
                  :class="t.state === 10 ? 'bg-success' : t.state > 10 ? 'bg-muted-foreground/40' : 'bg-primary'"
                  :style="{ width: t.percent + '%' }"
                />
              </div>
              <span class="w-24 shrink-0 text-muted-foreground">{{ t.progress }}</span>
            </div>
          </td>
          <td class="py-1.5 px-3">
            <Badge :variant="stateBadge(t.state).variant">{{ stateBadge(t.state).label }}</Badge>
          </td>
          <td class="py-1.5 px-3 font-mono text-muted-foreground hidden min-[1000px]:table-cell">
            {{ fmtTime(t.startTime) }}
          </td>
          <td class="py-1.5 px-3 font-mono text-muted-foreground hidden min-[1000px]:table-cell">
            {{ fmtTime(t.endTime) }}
          </td>
          <td class="py-1.5 px-3">
            <button
              v-if="t.state === 0"
              class="text-muted-foreground hover:text-destructive transition-colors"
              title="取消任务"
              @click="cancel(t.equipId, t.kind)"
            >
              <X class="h-3.5 w-3.5" />
            </button>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="h-full flex flex-col items-center justify-center gap-1.5 text-muted-foreground">
      <Inbox class="h-6 w-6 opacity-40" />
      <p class="text-xs">暂无升级 / 配置任务</p>
    </div>
  </div>
</template>

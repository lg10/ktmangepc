<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { useDeviceStore } from "@/stores/device";
import { useTaskStore } from "@/stores/task";
import { useHotelStore } from "@/stores/hotel";
import { useTelnetStore } from "@/stores/telnet";
import { useCheckinStore } from "@/stores/checkin";
import { useUiStore } from "@/stores/ui";
import { PanelBottom, Terminal, ListChecks, ScrollText } from "lucide-vue-next";

const deviceStore = useDeviceStore();
const taskStore = useTaskStore();
const hotelStore = useHotelStore();
const telnetStore = useTelnetStore();
const checkinStore = useCheckinStore();
const uiStore = useUiStore();

const modeLabel = computed(() => {
  switch (deviceStore.status.mode) {
    case 1:
      return "普通扫描";
    case 2:
      return "超级模式";
    case 3:
      return "DHCP 直连";
    case 4:
      return "门锁扫描";
    case 5:
      return "入住机扫描";
    default:
      return "";
  }
});

function jumpDock(tab: string) {
  uiStore.openDock(tab);
}

/** 右下角版本号：读 Tauri 配置（跟随 Cargo/conf 版本） */
const appVersion = ref("");
onMounted(async () => {
  try {
    appVersion.value = await getVersion();
  } catch {
    appVersion.value = "";
  }
});
</script>

<template>
  <footer
    class="h-6 shrink-0 border-t bg-muted/40 flex items-center gap-3 px-3 text-[11px] text-muted-foreground select-none"
  >
    <!-- 服务状态 -->
    <div class="flex items-center gap-1.5">
      <span
        class="h-1.5 w-1.5 rounded-full"
        :class="deviceStore.status.running ? 'bg-success animate-pulse' : 'bg-muted-foreground/40'"
      />
      <template v-if="deviceStore.status.running">
        <span class="text-foreground/80">{{ modeLabel }}</span>
        <span class="font-mono">{{ deviceStore.status.ip }}:{{ deviceStore.status.port }}</span>
      </template>
      <span v-else>服务未启动</span>
    </div>

    <span class="text-border">|</span>
    <span>{{ deviceStore.status.deviceCount }} 台设备</span>

    <!-- 入住机 mDNS 扫描服务（与 UDP 服务独立并行） -->
    <template v-if="checkinStore.running">
      <span class="text-border">|</span>
      <span class="flex items-center gap-1.5">
        <span class="h-1.5 w-1.5 rounded-full bg-success animate-pulse" />
        入住机扫描 · {{ checkinStore.deviceCount }} 台
      </span>
    </template>

    <button
      v-if="taskStore.activeCount > 0"
      class="flex items-center gap-1 hover:text-foreground transition-colors"
      title="查看任务"
      @click="jumpDock('tasks')"
    >
      <ListChecks class="h-3 w-3" />
      {{ taskStore.activeCount }} 个任务进行
    </button>

    <button
      v-if="telnetStore.tabs.length > 0"
      class="flex items-center gap-1 hover:text-foreground transition-colors"
      title="查看终端"
      @click="jumpDock(telnetStore.tabs[0] ? 'telnet:' + telnetStore.tabs[0].id : 'tasks')"
    >
      <Terminal class="h-3 w-3" />
      {{ telnetStore.tabs.length }} 终端
    </button>

    <div class="ml-auto flex items-center gap-3">
      <span v-if="hotelStore.state.synced" class="hidden min-[1180px]:inline">
        酒店已同步（{{ hotelStore.state.roomCount }} 房间）
      </span>
      <span class="hidden min-[1100px]:inline">肯天科技 · v{{ appVersion || "—" }}</span>
      <button
        class="flex items-center gap-1 hover:text-foreground transition-colors"
        :title="uiStore.dockOpen ? '收起底部面板' : '展开底部面板'"
        @click="uiStore.toggleDock()"
      >
        <ScrollText v-if="!uiStore.dockOpen" class="h-3 w-3" />
        <PanelBottom class="h-3.5 w-3.5" />
      </button>
    </div>
  </footer>
</template>

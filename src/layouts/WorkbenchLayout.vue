<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useUiStore } from "@/stores/ui";
import { useTaskStore } from "@/stores/task";
import { useHotelStore } from "@/stores/hotel";
import TitleBar from "@/components/layout/TitleBar.vue";
import SideNav from "@/components/layout/SideNav.vue";
import StatusBar from "@/components/layout/StatusBar.vue";
import Dock from "@/components/layout/Dock.vue";
import CommandPalette from "@/components/layout/CommandPalette.vue";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import FilesView from "@/views/FilesView.vue";
import SettingsView from "@/views/SettingsView.vue";

const uiStore = useUiStore();
const taskStore = useTaskStore();
const hotelStore = useHotelStore();

/** 响应式：窗口压缩时侧栏自动折叠为纯图标条，放宽后恢复 */
const autoCollapsed = ref(false);
const SIDEBAR_NARROW = 1240;
const SIDEBAR_WIDE = 1360;

function onResize() {
  const w = window.innerWidth;
  if (w < SIDEBAR_NARROW && !uiStore.sidebarCollapsed) {
    uiStore.setSidebar(true);
    autoCollapsed.value = true;
  } else if (w >= SIDEBAR_WIDE && autoCollapsed.value) {
    uiStore.setSidebar(false);
    autoCollapsed.value = false;
  }
}

function onKeydown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    uiStore.paletteOpen = !uiStore.paletteOpen;
  } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "j") {
    e.preventDefault();
    uiStore.toggleDock();
  }
}

onMounted(() => {
  window.addEventListener("resize", onResize);
  window.addEventListener("keydown", onKeydown);
  onResize();
  // 初始拉取：任务列表与酒店缓存
  taskStore.refresh().catch(() => {});
  hotelStore.loadState().catch(() => {});
  // 每次启动重新拉取酒店信息与授权信息，保证数据最新
  if (hotelStore.hotelId > 0) {
    hotelStore
      .sync()
      .then(() => hotelStore.fetchAuthInfo())
      .catch(() => {});
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", onResize);
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div class="h-screen w-screen flex flex-col overflow-hidden bg-background">
    <TitleBar />

    <div class="flex-1 flex min-h-0">
      <SideNav />

      <!-- 主列：内容 + 底部 Dock -->
      <div class="flex-1 min-w-0 flex flex-col">
        <main class="flex-1 min-h-0 overflow-hidden bg-background">
          <router-view />
        </main>
        <Dock v-if="uiStore.dockOpen" />
      </div>
    </div>

    <StatusBar />
    <CommandPalette />

    <!-- 文件库 / 设置：弹窗形式，不切路由，避免打断正在进行的设备扫描 -->
    <Dialog :open="uiStore.filesOpen" @update:open="uiStore.filesOpen = $event">
      <DialogContent class="max-w-5xl w-[86vw] h-[82vh] flex flex-col gap-0 p-0 overflow-hidden">
        <div class="shrink-0 flex items-center px-6 pt-4 pb-2">
          <DialogTitle class="text-base">文件库</DialogTitle>
        </div>
        <div class="flex-1 min-h-0 overflow-hidden">
          <FilesView />
        </div>
      </DialogContent>
    </Dialog>

    <Dialog :open="uiStore.settingsOpen" @update:open="uiStore.settingsOpen = $event">
      <DialogContent class="max-w-4xl w-[76vw] h-[82vh] flex flex-col gap-0 p-0 overflow-hidden">
        <DialogTitle class="sr-only">设置</DialogTitle>
        <div class="flex-1 min-h-0 overflow-hidden pt-6">
          <SettingsView />
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>

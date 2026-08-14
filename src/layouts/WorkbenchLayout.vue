<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useUiStore } from "@/stores/ui";
import { useTaskStore } from "@/stores/task";
import { useHotelStore } from "@/stores/hotel";
import { api, EVENTS } from "@/lib/api";
import { restoreNetwork } from "@/lib/exitGuard";
import { useToast } from "@/components/ui/toast/use-toast";
import TitleBar from "@/components/layout/TitleBar.vue";
import SideNav from "@/components/layout/SideNav.vue";
import StatusBar from "@/components/layout/StatusBar.vue";
import Dock from "@/components/layout/Dock.vue";
import CommandPalette from "@/components/layout/CommandPalette.vue";
import WelcomeGreeting from "@/components/layout/WelcomeGreeting.vue";
import ExitGuardDialog from "@/components/ExitGuardDialog.vue";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import FilesView from "@/views/FilesView.vue";
import SettingsView from "@/views/SettingsView.vue";

const uiStore = useUiStore();
const taskStore = useTaskStore();
const hotelStore = useHotelStore();
const { toast } = useToast();

/** 启动残留检测：被 kill / 强制关机后可能残留共享开关与 134.1 地址，弹窗询问是否恢复 */
const residueOpen = ref(false);
const residueDetail = ref("");
let unlistenExit: UnlistenFn | null = null;

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

  // 退出拦截：关窗时 DHCP / 中继仍在运行，后端拦下关闭并发事件，先恢复再销毁窗口
  listen(EVENTS.EXIT_BLOCKED, () => {
    restoreNetwork(() => getCurrentWindow().destroy());
  }).then((u) => (unlistenExit = u));

  // 启动残留检测（每个会话只提示一次）：上次异常退出可能遗留网络配置
  if (!sessionStorage.getItem("kt.residueChecked")) {
    sessionStorage.setItem("kt.residueChecked", "1");
    api
      .checkNicResidue()
      .then((r) => {
        if (!r.markerFound && !r.sharingEnabled && r.residualNics.length === 0) return;
        const parts: string[] = [];
        if (r.markerFound) parts.push("上次运行未正常退出");
        if (r.sharingEnabled) parts.push("系统互联网共享仍处于开启");
        if (r.residualNics.length) {
          parts.push(`网卡 ${r.residualNics.join("、")} 残留 192.168.134.1 地址`);
        }
        residueDetail.value = parts.join("；") + "，建议立即恢复以免影响上网。";
        residueOpen.value = true;
      })
      .catch(() => {});
  }
});

/** 立即恢复残留（提权助手清理，需系统管理员授权） */
async function restoreResidue() {
  try {
    const msg = await api.restoreNetwork();
    toast({ title: msg, variant: "success" });
  } catch (e) {
    toast({ title: "恢复失败", description: String(e), variant: "destructive" });
  }
}

onBeforeUnmount(() => {
  window.removeEventListener("resize", onResize);
  window.removeEventListener("keydown", onKeydown);
  unlistenExit?.();
});
</script>

<template>
  <div class="h-screen w-screen flex flex-col overflow-hidden bg-background">
    <!-- 进入工作台的欢迎语动画（登录进入/静默恢复均展示，约 2s 后自动淡出） -->
    <WelcomeGreeting />
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

    <!-- 退出 / 退出登录等待弹窗：DHCP / 中继恢复完成后才放行 -->
    <ExitGuardDialog />

    <!-- 启动残留检测：上次异常退出的网络配置残留，询问是否恢复 -->
    <ConfirmDialog
      v-model:open="residueOpen"
      title="检测到网络配置残留"
      :description="residueDetail"
      confirm-text="立即恢复"
      cancel-text="暂不"
      @confirm="restoreResidue"
    />

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

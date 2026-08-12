<script setup lang="ts">
import { useRoute, useRouter } from "vue-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAuthStore } from "@/stores/auth";
import { useDeviceStore } from "@/stores/device";
import { useUiStore } from "@/stores/ui";
import { api } from "@/lib/api";
import { openLoginWindow } from "@/lib/window";
import {
  Rocket,
  FolderArchive,
  Settings,
  LogOut,
  PanelLeftClose,
  PanelLeft,
} from "lucide-vue-next";

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const deviceStore = useDeviceStore();
const uiStore = useUiStore();

const navItems = [
  { name: "launch", label: "工作模式", icon: Rocket },
  { name: "files", label: "文件库", icon: FolderArchive },
  { name: "settings", label: "设置", icon: Settings },
];

/** 工作模式走路由；文件库/设置开弹窗，避免路由切换打断设备扫描 */
function navigate(name: string) {
  if (name === "launch") {
    uiStore.filesOpen = false;
    uiStore.settingsOpen = false;
    router.push({ name: "launch" });
  } else if (name === "files") {
    uiStore.filesOpen ? (uiStore.filesOpen = false) : uiStore.openFiles();
  } else if (name === "settings") {
    uiStore.settingsOpen ? (uiStore.settingsOpen = false) : uiStore.openSettings();
  }
}

function isActive(name: string) {
  if (name === "launch")
    return route.name === "launch" && !uiStore.filesOpen && !uiStore.settingsOpen;
  if (name === "files") return uiStore.filesOpen;
  return uiStore.settingsOpen;
}

async function logout() {
  if (deviceStore.status.running) {
    await api.stopUdpServer().catch(() => {});
  }
  await authStore.logout();
  // 微信式切窗：开登录窗后关闭工作台
  await openLoginWindow();
  await new Promise((r) => setTimeout(r, 250));
  getCurrentWindow().close();
}
</script>

<template>
  <aside
    class="shrink-0 border-r bg-muted/30 flex flex-col transition-[width] duration-150 overflow-hidden"
    :class="uiStore.sidebarCollapsed ? 'w-[52px]' : 'w-[190px]'"
  >
    <nav class="flex-1 py-2 px-2 space-y-0.5">
      <button
        v-for="item in navItems"
        :key="item.name"
        :title="uiStore.sidebarCollapsed ? item.label : ''"
        class="w-full flex items-center gap-2.5 rounded-md text-[13px] transition-colors"
        :class="[
          uiStore.sidebarCollapsed ? 'justify-center px-0 py-2' : 'px-2.5 py-2',
          isActive(item.name)
            ? 'bg-primary/10 text-primary font-medium'
            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
        ]"
        @click="navigate(item.name)"
      >
        <component :is="item.icon" class="h-4 w-4 shrink-0" />
        <span v-if="!uiStore.sidebarCollapsed" class="truncate">{{ item.label }}</span>
      </button>
    </nav>

    <!-- 底部：用户 + 折叠 -->
    <div class="px-2 pb-2 space-y-0.5 border-t pt-2">
      <div
        class="flex items-center gap-2.5 rounded-md px-2.5 py-1.5"
        :class="uiStore.sidebarCollapsed && 'justify-center px-0'"
        :title="uiStore.sidebarCollapsed ? authStore.nickName : ''"
      >
        <div
          class="h-6 w-6 shrink-0 rounded-full bg-primary/15 text-primary flex items-center justify-center text-[11px] font-semibold"
        >
          {{ authStore.nickName?.charAt(0) || "U" }}
        </div>
        <div v-if="!uiStore.sidebarCollapsed" class="min-w-0 flex-1">
          <div class="text-xs font-medium truncate">{{ authStore.nickName }}</div>
        </div>
        <button
          v-if="!uiStore.sidebarCollapsed"
          class="text-muted-foreground hover:text-destructive transition-colors"
          title="退出登录"
          @click="logout"
        >
          <LogOut class="h-3.5 w-3.5" />
        </button>
      </div>
      <button
        v-if="uiStore.sidebarCollapsed"
        class="w-full flex justify-center py-1.5 text-muted-foreground hover:text-foreground"
        title="退出登录"
        @click="logout"
      >
        <LogOut class="h-4 w-4" />
      </button>
      <button
        class="w-full flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
        :class="uiStore.sidebarCollapsed && 'justify-center px-0'"
        :title="uiStore.sidebarCollapsed ? '展开侧栏' : ''"
        @click="uiStore.toggleSidebar()"
      >
        <component :is="uiStore.sidebarCollapsed ? PanelLeft : PanelLeftClose" class="h-4 w-4 shrink-0" />
        <span v-if="!uiStore.sidebarCollapsed">收起侧栏</span>
      </button>
    </div>
  </aside>
</template>

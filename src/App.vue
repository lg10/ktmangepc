<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { EVENTS } from "@/lib/api";
import { openWorkbenchWindow } from "@/lib/window";
import { useDeviceStore } from "@/stores/device";
import { useAuthStore } from "@/stores/auth";
import { useTaskStore } from "@/stores/task";
import { useTelnetStore } from "@/stores/telnet";
import { useFileStore } from "@/stores/file";
import { useUiStore } from "@/stores/ui";
import type {
  FetchProgress,
  LockPacket,
  RcuDevice,
  ServerStatus,
  UpdateTask,
  UserInfo,
} from "@/types";
import Toaster from "@/components/ui/toast/Toaster.vue";

const deviceStore = useDeviceStore();
const authStore = useAuthStore();
const taskStore = useTaskStore();
const telnetStore = useTelnetStore();
const fileStore = useFileStore();
const uiStore = useUiStore();

let unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  unlisteners = await Promise.all([
    listen<RcuDevice>(EVENTS.DEVICE, (e) => deviceStore.upsertDevice(e.payload)),
    listen<string>(EVENTS.DEVICE_OFFLINE, (e) =>
      deviceStore.removeDevice(e.payload)
    ),
    listen<LockPacket>(EVENTS.LOCK_PACKET, (e) =>
      deviceStore.pushLockPacket(e.payload)
    ),
    listen<ServerStatus>(EVENTS.SERVER_STATUS, (e) =>
      deviceStore.setStatus(e.payload)
    ),
    listen<string>(EVENTS.UDP_LOG, (e) => deviceStore.pushLog(e.payload)),
    listen<UserInfo>(EVENTS.LOGIN_SUCCESS, async (e) => {
      authStore.setUser(e.payload);
      // 微信式切窗：新开工作台窗口（显式登录必经酒店选择页），待其创建后关闭登录窗
      const wb = await openWorkbenchWindow(true);
      await Promise.race([
        new Promise<void>((r) => wb.once("tauri://created", () => r())),
        new Promise<void>((r) => setTimeout(r, 1500)),
      ]);
      setTimeout(() => getCurrentWindow().close(), 250);
    }),
    // 阶段二：拉取进度 / 任务更新 / Telnet 会话事件
    listen<FetchProgress>(EVENTS.FILE_FETCH_PROGRESS, (e) => {
      fileStore.setProgress(e.payload);
      if (e.payload.stage !== "done" && e.payload.stage !== "error") {
        uiStore.openDock("logs");
      }
    }),
    listen<UpdateTask[]>(EVENTS.TASK_UPDATE, (e) => {
      taskStore.setTasks(e.payload);
      if (e.payload.some((t) => t.state <= 1) && !uiStore.dockOpen) {
        uiStore.openDock("tasks");
      }
    }),
    listen<{ id: string; reason: string }>(EVENTS.TELNET_CLOSED, (e) =>
      telnetStore.markClosed(e.payload.id, e.payload.reason)
    ),
    // 收到设备回包即为真实已连接（兼容 invoke 返回时序，避免卡在“连接中”）
    listen<{ id: string; data: string }>(EVENTS.TELNET_DATA, (e) =>
      telnetStore.markConnected(e.payload.id)
    ),
  ]);

  // RCU 模拟器：仅 VITE_MOCK_RCU=1 启动 dev 时动态加载，正常打包/开发零影响
  if (import.meta.env.VITE_MOCK_RCU === "1") {
    void import("@/lib/mockRcu").then((m) => m.startMockRcu());
  }
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
});
</script>

<template>
  <router-view />
  <Toaster />
</template>

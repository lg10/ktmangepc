<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { api } from "@/lib/api";
import { useCheckinStore } from "@/stores/checkin";
import { checkinAdminUrl, checkinBase, checkinDeviceInfo } from "@/lib/checkin";
import { openAdminWindow } from "@/lib/window";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import CheckinPasswordDialog from "@/components/CheckinPasswordDialog.vue";
import { Square, Globe, KeyRound, RefreshCw, TabletSmartphone } from "lucide-vue-next";
import type { CheckinDevice } from "@/types";

/**
 * 入住机扫描页：mDNS 持续发现（后端事件驱动），列表展示基础信息与登录后的详情；
 * 「管理页面」在应用内新窗口打开设备管理台，地球图标可转系统浏览器。
 */
const router = useRouter();
const checkinStore = useCheckinStore();
const { toast } = useToast();

const stopping = ref(false);
const passwordDialogOpen = ref(false);
const passwordDevice = ref<CheckinDevice | null>(null);

onMounted(async () => {
  // 非运行态直达本页（如手动输地址）：回启动页
  const st = await api.checkinStatus().catch(() => null);
  if (!st || !st.running) {
    router.replace({ name: "launch" });
    return;
  }
  checkinStore.setStatus(st);
  checkinStore.setDevices(await api.checkinList().catch(() => []));
  // 已登录设备刷新详情（IP 可能随 mDNS 重新解析变化）
  for (const d of checkinStore.devices) refreshInfo(d);
});

onBeforeUnmount(() => {
  /* 页面离开不停止服务（与监控页一致），停止仅在点击「停止扫描」时 */
});

async function stop() {
  stopping.value = true;
  try {
    await api.checkinStop();
    checkinStore.reset();
    router.push({ name: "launch" });
  } catch (e) {
    toast({ title: "停止失败", description: String(e), variant: "destructive" });
  } finally {
    stopping.value = false;
  }
}

function sessionOf(d: CheckinDevice) {
  return checkinStore.sessions[d.fullName] ?? null;
}

/** 打开管理台：应用内新窗口（顶栏地球图标转系统浏览器） */
function openAdmin(d: CheckinDevice) {
  openAdminWindow(checkinAdminUrl(d), `入住机管理台 · ${d.name}`).catch((e) =>
    toast({ title: "打开管理页失败", description: String(e), variant: "destructive" })
  );
}

function login(d: CheckinDevice) {
  passwordDevice.value = d;
  passwordDialogOpen.value = true;
}

/** 已登录设备刷新详情；token 过期时清除会话引导重新登录 */
async function refreshInfo(d: CheckinDevice) {
  const session = sessionOf(d);
  if (!session) return;
  try {
    const info = await checkinDeviceInfo(checkinBase(d), session.token);
    checkinStore.setSessionInfo(d.fullName, info);
  } catch (e) {
    if (e instanceof Error && e.message === "TOKEN_EXPIRED") {
      checkinStore.clearSession(d.fullName);
    }
  }
}
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- 工具条 -->
    <div class="shrink-0 flex items-center gap-3 px-6 pt-5 pb-3">
      <div class="h-9 w-9 rounded-lg bg-primary/10 text-primary flex items-center justify-center">
        <TabletSmartphone class="h-4.5 w-4.5" />
      </div>
      <div class="min-w-0">
        <h2 class="text-base font-semibold leading-tight">入住机扫描</h2>
        <p class="text-xs text-muted-foreground">
          mDNS 持续发现 _kingint-kcd._tcp 服务（端口 7271）
        </p>
      </div>
      <div class="ml-auto flex items-center gap-3">
        <span class="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span class="h-1.5 w-1.5 rounded-full bg-success animate-pulse" />
          {{ checkinStore.deviceCount }} 台设备
        </span>
        <Button variant="destructive" size="sm" :disabled="stopping" @click="stop">
          <Square class="h-3.5 w-3.5" />
          停止扫描
        </Button>
      </div>
    </div>

    <!-- 设备列表 -->
    <div class="flex-1 min-h-0 overflow-auto px-6 pb-6">
      <div class="rounded-lg border overflow-hidden">
        <table class="w-full text-[13px]">
          <thead>
            <tr class="border-b bg-muted/40 text-left text-xs text-muted-foreground">
              <th class="px-3 py-2 font-medium">服务名</th>
              <th class="px-3 py-2 font-medium">IP</th>
              <th class="px-3 py-2 font-medium">端口</th>
              <th class="px-3 py-2 font-medium">设备 ID</th>
              <th class="px-3 py-2 font-medium">型号</th>
              <th class="px-3 py-2 font-medium">版本</th>
              <th class="px-3 py-2 font-medium text-right">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="checkinStore.devices.length === 0">
              <td colspan="7" class="px-3 py-10 text-center text-sm text-muted-foreground">
                正在搜索局域网内的入住机，请确认与设备处于同一网段…
              </td>
            </tr>
            <tr
              v-for="d in checkinStore.devices"
              :key="d.fullName"
              class="border-b last:border-0 hover:bg-accent/40 transition-colors"
            >
              <td class="px-3 py-2 font-medium">
                <div class="flex items-center gap-1.5">
                  {{ d.name }}
                  <Badge v-if="sessionOf(d)" variant="secondary" class="text-[10px]">已登录</Badge>
                </div>
              </td>
              <td class="px-3 py-2 font-mono">{{ d.ip }}</td>
              <td class="px-3 py-2 font-mono">{{ d.port }}</td>
              <td class="px-3 py-2 text-muted-foreground">
                {{ sessionOf(d)?.info?.deviceId ?? "—" }}
              </td>
              <td class="px-3 py-2 text-muted-foreground">
                {{ sessionOf(d)?.info?.model ?? "—" }}
              </td>
              <td class="px-3 py-2 text-muted-foreground">
                {{ sessionOf(d)?.info?.version ?? "—" }}
              </td>
              <td class="px-3 py-2">
                <div class="flex items-center justify-end gap-1">
                  <Button
                    variant="outline"
                    size="sm"
                    class="h-7 gap-1"
                    title="应用内打开管理台"
                    @click="openAdmin(d)"
                  >
                    <Globe class="h-3.5 w-3.5" />
                    管理页面
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    class="h-7 gap-1"
                    @click="login(d)"
                  >
                    <KeyRound class="h-3.5 w-3.5" />
                    {{ sessionOf(d) ? "重新登录" : "登录" }}
                  </Button>
                  <Button
                    v-if="sessionOf(d)"
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    title="刷新设备信息"
                    @click="refreshInfo(d)"
                  >
                    <RefreshCw class="h-3.5 w-3.5" />
                  </Button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <CheckinPasswordDialog v-model:open="passwordDialogOpen" :device="passwordDevice" />
  </div>
</template>

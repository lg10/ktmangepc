<script setup lang="ts">
import { computed, markRaw, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api, EVENTS } from "@/lib/api";
import { useDeviceStore } from "@/stores/device";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Loader2 } from "lucide-vue-next";
import type { DhcpLease, DhcpStatus } from "@/types";

const deviceStore = useDeviceStore();
const { toast } = useToast();

/** 当前应用版本（读 Tauri 配置，非 Tauri 环境回退） */
const appVersion = ref("");
const checking = ref(false);
const checked = ref(false);
/** 检查到的待安装包（Tauri Updater，含签名校验） */
const pendingUpdate = ref<Update | null>(null);
const installing = ref(false);
const dlTotal = ref(0);
const dlDone = ref(0);

const dhcp = ref<DhcpStatus>({
  running: false,
  interfaceName: "",
  serverIp: "",
  leaseCount: 0,
  autoMode: false,
});
const leases = ref<DhcpLease[]>([]);
/** 智能模式：检测到真实网络自动跳过（仅网线直连离线场景启用），持久化 */
const dhcpAuto = ref(localStorage.getItem("kt.dhcpAuto") !== "0");
function setDhcpAuto(v: boolean) {
  dhcpAuto.value = v;
  localStorage.setItem("kt.dhcpAuto", v ? "1" : "0");
}
let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  try {
    appVersion.value = await getVersion();
  } catch {
    appVersion.value = "3.0.1";
  }
  await refresh();
  unlisten = await listen<DhcpLease>(EVENTS.DHCP_LEASE, (e) => {
    leases.value.push(e.payload);
    if (leases.value.length > 100) leases.value.shift();
  });
});

onUnmounted(() => {
  unlisten?.();
});

async function refresh() {
  try {
    dhcp.value = await api.dhcpStatus();
  } catch {
    /* ignore */
  }
}

async function toggleDhcp(v: boolean) {
  try {
    if (v) {
      const nic = deviceStore.interfaces.find(
        (i) => i.ip === deviceStore.selectedIp
      );
      await api.startDhcp(nic?.name ?? "", dhcpAuto.value);
      toast({ title: "DHCP 服务已启动", variant: "success" });
    } else {
      await api.stopDhcp();
      toast({ title: "DHCP 服务已停止" });
    }
    await refresh();
  } catch (e) {
    dhcp.value.running = false;
    toast({
      title: "DHCP 启动失败",
      description: `${e}（需要管理员/root 权限绑定 67 端口）`,
      variant: "destructive",
    });
  }
}

const leaseList = computed(() => [...leases.value].reverse());

function fmtTime(ts: number) {
  return new Date(ts).toLocaleTimeString("zh-CN", { hour12: false });
}

/** 检查更新：请求云端 latest.json（Tauri Updater 清单），比较版本并取签名安装包 */
async function checkUpdate() {
  if (checking.value) return;
  checking.value = true;
  try {
    const update = await check();
    checked.value = true;
    if (update) {
      // Update 实例含 # 私有字段，必须 markRaw 阻止 Vue 响应式代理包装，
      // 否则调用 downloadAndInstall 时报 Cannot read private member
      pendingUpdate.value = markRaw(update);
      toast({ title: `发现新版本 v${update.version}`, variant: "success" });
    } else {
      pendingUpdate.value = null;
      toast({ title: "当前已是最新版本", variant: "success" });
    }
  } catch (e) {
    toast({
      title: "检查更新失败",
      description: String(e),
      variant: "destructive",
    });
  } finally {
    checking.value = false;
  }
}

const progressText = computed(() => {
  if (!dlTotal.value) return "";
  return `${Math.min(99, Math.round((dlDone.value / dlTotal.value) * 100))}%`;
});

/** 立即更新：下载签名安装包 → 校验 → 静默安装 → 自动重启 */
async function installUpdate() {
  if (!pendingUpdate.value || installing.value) return;
  installing.value = true;
  dlTotal.value = 0;
  dlDone.value = 0;
  try {
    await pendingUpdate.value.downloadAndInstall((e) => {
      if (e.event === "Started" && e.data.contentLength) {
        dlTotal.value = e.data.contentLength;
      } else if (e.event === "Progress") {
        dlDone.value += e.data.chunkLength;
      }
    });
    toast({ title: "下载完成，正在重启安装…", variant: "success" });
    await relaunch();
  } catch (e) {
    installing.value = false;
    toast({
      title: "更新安装失败",
      description: String(e),
      variant: "destructive",
    });
  }
}
</script>

<template>
  <div class="h-full overflow-auto">
  <div class="p-8 max-w-3xl mx-auto space-y-6">
    <div>
      <h2 class="text-xl font-semibold">设置</h2>
      <p class="mt-1 text-sm text-muted-foreground">DHCP 服务与运行时日志</p>
    </div>

    <!-- 版本与更新 -->
    <Card class="p-5">
      <div class="flex items-center justify-between">
        <div>
          <div class="font-medium text-sm">肯天玉佩</div>
          <div class="text-xs text-muted-foreground mt-1">
            当前版本 v{{ appVersion || "—" }}
            <span v-if="checked && !pendingUpdate" class="ml-2 text-emerald-600 dark:text-emerald-400">已是最新</span>
          </div>
        </div>
        <Button variant="outline" size="sm" :disabled="checking || installing" @click="checkUpdate">
          <Loader2 v-if="checking" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
          {{ checking ? "检查中…" : "检查更新" }}
        </Button>
      </div>
      <div
        v-if="pendingUpdate"
        class="mt-4 rounded-md border border-primary/30 bg-primary/5 p-3"
      >
        <div class="text-xs font-medium">
          发现新版本 v{{ pendingUpdate.version }}
        </div>
        <div
          v-if="pendingUpdate.body"
          class="mt-1.5 text-xs text-muted-foreground whitespace-pre-wrap"
        >
          {{ pendingUpdate.body }}
        </div>
        <Button
          size="sm"
          class="mt-2.5 h-7 text-xs"
          :disabled="installing"
          @click="installUpdate"
        >
          <Loader2 v-if="installing" class="mr-1.5 h-3 w-3 animate-spin" />
          {{ installing ? `正在下载… ${progressText}` : "立即更新（自动安装并重启）" }}
        </Button>
      </div>
    </Card>

    <!-- DHCP 管理 -->
    <Card class="p-5">
      <div class="flex items-center justify-between">
        <div>
          <div class="font-medium text-sm">内置 DHCP 服务器</div>
          <div class="text-xs text-muted-foreground mt-1">
            为网线直连设备自动分配 192.168.134.x 段 IP
          </div>
        </div>
        <div class="flex items-center gap-3">
          <Badge v-if="dhcp.running" variant="success">运行中 {{ dhcp.serverIp }}</Badge>
          <Badge v-else variant="secondary">已停止</Badge>
          <Switch :model-value="dhcp.running" @update:model-value="toggleDhcp" />
        </div>
      </div>

      <div class="mt-4 flex items-center justify-between">
        <div>
          <div class="text-xs font-medium">智能模式</div>
          <div class="text-[11px] text-muted-foreground mt-0.5">
            检测到真实网络时自动跳过，仅网线直连的离线场景启用
          </div>
        </div>
        <Switch :model-value="dhcpAuto" @update:model-value="setDhcpAuto" />
      </div>

      <template v-if="dhcp.running">
        <Separator class="my-4" />
        <div class="text-xs text-muted-foreground mb-2">
          已分配租约（{{ leaseList.length }}）
        </div>
        <div class="max-h-40 overflow-auto">
          <table class="w-full text-xs">
            <tbody>
              <tr v-for="(l, i) in leaseList" :key="i" class="border-b border-border/50">
                <td class="py-1.5 pr-4 font-mono">{{ l.mac }}</td>
                <td class="py-1.5 pr-4 font-mono">{{ l.ip }}</td>
                <td class="py-1.5 text-muted-foreground">{{ fmtTime(l.ts) }}</td>
              </tr>
            </tbody>
          </table>
          <div v-if="leaseList.length === 0" class="text-xs text-muted-foreground py-2">
            暂无租约
          </div>
        </div>
      </template>
    </Card>

    <!-- 运行日志 -->
    <Card class="p-5">
      <div class="flex items-center justify-between mb-3">
        <div class="font-medium text-sm">运行日志</div>
        <Button variant="ghost" size="sm" @click="deviceStore.logs = []">清空</Button>
      </div>
      <div
        class="h-64 overflow-auto rounded-md bg-muted/40 p-3 font-mono text-[11px] leading-relaxed select-text"
      >
        <div v-for="(l, i) in deviceStore.logs" :key="i" class="whitespace-pre-wrap break-all">
          {{ l }}
        </div>
        <div v-if="deviceStore.logs.length === 0" class="text-muted-foreground">
          暂无日志
        </div>
      </div>
    </Card>
  </div>
  </div>
</template>

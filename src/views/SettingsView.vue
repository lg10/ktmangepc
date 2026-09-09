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
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import { Loader2, RefreshCw } from "lucide-vue-next";
import type { DhcpLease, DhcpStatus, InetShareStatus, NetInterface } from "@/types";

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
/** DHCP 启停中（后端同步等待网卡配置/助手还原，返回即最新状态） */
const dhcpBusy = ref<"" | "starting" | "stopping">("");
/** 智能模式：检测到真实网络自动跳过（仅网线直连离线场景启用），持久化 */
const dhcpAuto = ref(localStorage.getItem("kt.dhcpAuto") === "1");
function setDhcpAuto(v: boolean) {
  dhcpAuto.value = v;
  localStorage.setItem("kt.dhcpAuto", v ? "1" : "0");
}
let unlisten: UnlistenFn | null = null;
let unlistenInet: UnlistenFn | null = null;

/** 网络中继（互联网共享）：把源网卡的网络共享给目标网口，给直连设备供网 */
const inetShare = ref<InetShareStatus>({ running: false, src: "", dst: "" });
const inetBusy = ref<"" | "starting" | "stopping">("");
const inetSrc = ref("");
const inetDst = ref("");
/** 源网卡候选：链路已连接且持有 IPv4（即当前有网络的网卡） */
const srcNics = computed(() =>
  nicList.value.filter((n) => n.up && n.hasIpv4 && n.name !== inetDst.value)
);
/** 目标网口候选：除源网卡外全部网口（允许选断开网口，可先选后插网线） */
const dstNics = computed(() =>
  nicList.value.filter((n) => n.name !== inetSrc.value)
);

async function toggleInetShare(v: boolean) {
  if (!v) {
    inetBusy.value = "stopping";
    try {
      await api.stopInetShare();
      inetShare.value = await api.inetShareStatus();
    } catch (e) {
      toast({ title: "停止失败", description: String(e), variant: "destructive" });
    } finally {
      inetBusy.value = "";
    }
    return;
  }
  if (!inetSrc.value || !inetDst.value) {
    toast({ title: "请先选择源网卡和目标网口", variant: "destructive" });
    return;
  }
  inetBusy.value = "starting";
  try {
    await api.startInetShare(inetSrc.value, inetDst.value);
    inetShare.value = await api.inetShareStatus();
  } catch (e) {
    toast({ title: "开启共享失败", description: String(e), variant: "destructive" });
  } finally {
    inetBusy.value = "";
  }
}

/** 确认弹窗：真实网络风险（手动启动 DHCP 时检测到非离线环境） */
const confirmRealNetwork = ref(false);
const realNetworkMsg = ref("");

/** 一键恢复网络原状：提权助手清理异常退出残留（共享开关 / 134.1 地址 / 静态模式） */
const restoring = ref(false);
async function restoreNet() {
  if (restoring.value) return;
  restoring.value = true;
  try {
    const msg = await api.restoreNetwork();
    toast({ title: msg, variant: "success" });
    deviceStore.loadInterfaces();
  } catch (e) {
    toast({ title: "恢复失败", description: String(e), variant: "destructive" });
  } finally {
    restoring.value = false;
  }
}

/** DHCP 网卡选择：独立于首页工作网卡（selectedNic），两者互不影响 */
const confirmNic = ref(false);
const nicList = ref<NetInterface[]>([]);
const dhcpNic = ref("");
const nicLoading = ref(false);

async function reloadNics() {
  nicLoading.value = true;
  try {
    nicList.value = await api.listInterfaces();
  } catch {
    nicList.value = [];
  } finally {
    nicLoading.value = false;
  }
}

async function openNicDialog() {
  confirmNic.value = true;
  await reloadNics();
  // 默认选中：正在运行的网卡 > 第一块 Up 网卡
  dhcpNic.value =
    (dhcp.value.running ? dhcp.value.interfaceName : "") ||
    nicList.value.find((n) => n.up)?.name ||
    "";
}

onMounted(async () => {
  try {
    appVersion.value = await getVersion();
  } catch {
    appVersion.value = "3.0.1";
  }
  await Promise.all([refresh(), reloadNics()]);
  unlisten = await listen<DhcpLease>(EVENTS.DHCP_LEASE, (e) => {
    // 按 MAC 去重：同一设备重复请求只保留一行，避免刷屏
    const i = leases.value.findIndex((l) => l.mac === e.payload.mac);
    if (i >= 0) leases.value.splice(i, 1);
    leases.value.push(e.payload);
  });
  unlistenInet = await listen<InetShareStatus>(EVENTS.INET_STATUS, (e) => {
    inetShare.value = e.payload;
  });
});

onUnmounted(() => {
  unlisten?.();
  unlistenInet?.();
  // 离开设置页时刷新首页网卡列表（开关 DHCP 会改变网卡 IP 展示）
  deviceStore.loadInterfaces();
});

async function refresh() {
  try {
    dhcp.value = await api.dhcpStatus();
  } catch {
    /* ignore */
  }
  try {
    inetShare.value = await api.inetShareStatus();
  } catch {
    /* ignore */
  }
  await loadLeases();
}

/** 拉取持久化租约：后端 ARP 存活探测，只展示当前真实在线的分配记录 */
const leasesLoading = ref(false);
async function loadLeases() {
  leasesLoading.value = true;
  try {
    leases.value = await api.dhcpLeases();
  } catch {
    /* ignore */
  } finally {
    leasesLoading.value = false;
  }
}

async function toggleDhcp(v: boolean) {
  if (v) {
    // 启动前先选网卡（独立于首页工作网卡），取消则保持停止状态
    await openNicDialog();
    return;
  }
  dhcpBusy.value = "stopping";
  try {
    await api.stopDhcp();
    toast({ title: "DHCP 服务已停止" });
    await refresh();
    // 停止会移除 134.1：同步刷新首页网卡列表（后端已等助手还原完毕）
    deviceStore.loadInterfaces();
  } catch (e) {
    toast({
      title: "DHCP 停止失败",
      description: String(e),
      variant: "destructive",
    });
  } finally {
    dhcpBusy.value = "";
  }
}

/** 网卡对话框确认：用所选网卡启动 DHCP */
async function confirmNicSelected() {
  if (!dhcpNic.value) {
    toast({ title: "请先选择网卡", variant: "destructive" });
    return;
  }
  try {
    await doStartDhcp(false);
  } catch (e) {
    toast({
      title: "DHCP 启动失败",
      description: String(e),
      variant: "destructive",
    });
  }
}

/** 启动 DHCP：未提权时后端会弹系统密码框拉起特权助手中继，无需重启应用 */
async function doStartDhcp(force: boolean) {
  dhcpBusy.value = "starting";
  try {
    await api.startDhcp(dhcpNic.value, dhcpAuto.value, force);
    toast({ title: "DHCP 服务已启动", variant: "success" });
    await refresh();
    // 启动会添加 134.1：同步刷新首页网卡列表
    deviceStore.loadInterfaces();
  } catch (e) {
    const msg = String(e);
    if (msg.startsWith("REAL_NETWORK:")) {
      // 转交风险确认弹窗（点「仍然启动」会再次进入本函数）
      realNetworkMsg.value = msg.slice("REAL_NETWORK:".length);
      confirmRealNetwork.value = true;
      return;
    }
    throw e;
  } finally {
    dhcpBusy.value = "";
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
    // 拆分 download/install： updater 在 Windows 拉起安装器后直接硬退出进程
    // （不走 RunEvent::Exit 清理），故在下载完成后、install 前主动释放
    // adb server 常驻守护，避免安装目录被句柄锁定弹“无法写入”
    await pendingUpdate.value.download((e) => {
      if (e.event === "Started" && e.data.contentLength) {
        dlTotal.value = e.data.contentLength;
      } else if (e.event === "Progress") {
        dlDone.value += e.data.chunkLength;
      }
    });
    await api.adbKillServer().catch(() => {});
    await pendingUpdate.value.install();
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
          <div class="font-medium text-sm">Device Scan</div>
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
          <Badge v-if="dhcp.running" variant="success">
            运行中 {{ dhcp.interfaceName }}（{{ dhcp.serverIp }}）
          </Badge>
          <Badge v-else-if="dhcpBusy === 'starting'" variant="secondary">
            <Loader2 class="mr-1 h-3 w-3 animate-spin" />DHCP 启用中…
          </Badge>
          <Badge v-else-if="dhcpBusy === 'stopping'" variant="secondary">
            <Loader2 class="mr-1 h-3 w-3 animate-spin" />正在停止…
          </Badge>
          <Badge v-else variant="secondary">已停止</Badge>
          <Switch
            :model-value="dhcp.running"
            :disabled="dhcpBusy !== '' || (!dhcp.running && inetShare.running)"
            @update:model-value="toggleDhcp"
          />
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
          <div
            v-if="leaseList.length === 0"
            class="text-xs text-muted-foreground py-2 flex items-center gap-1.5"
          >
            <Loader2 v-if="leasesLoading" class="h-3 w-3 animate-spin" />
            {{ leasesLoading ? "正在探测设备在线状态，自动加载中…" : "暂无租约" }}
          </div>
        </div>
      </template>
    </Card>

    <!-- 网络中继（互联网共享） -->
    <Card class="p-5">
      <div class="flex items-center justify-between">
        <div>
          <div class="font-medium text-sm">网络中继（给设备供网）</div>
          <div class="text-xs text-muted-foreground mt-1">
            用系统内置共享把源网卡（如 Wi-Fi）的互联网中继给目标网口，设备即可上网；
            设备 IP 由系统分配（Windows 192.168.137.x / macOS 192.168.2.x），与内置 DHCP 互斥
          </div>
        </div>
        <div class="flex items-center gap-3">
          <Badge v-if="inetShare.running" variant="success">
            共享中 {{ inetShare.src }} → {{ inetShare.dst }}
          </Badge>
          <Badge v-else-if="inetBusy === 'starting'" variant="secondary">
            <Loader2 class="mr-1 h-3 w-3 animate-spin" />共享开启中…
          </Badge>
          <Badge v-else-if="inetBusy === 'stopping'" variant="secondary">
            <Loader2 class="mr-1 h-3 w-3 animate-spin" />正在停止…
          </Badge>
          <Badge v-else variant="secondary">已停止</Badge>
          <Switch
            :model-value="inetShare.running"
            :disabled="inetBusy !== '' || (!inetShare.running && dhcp.running)"
            @update:model-value="toggleInetShare"
          />
        </div>
      </div>

      <template v-if="!inetShare.running">
        <div class="mt-4 grid grid-cols-2 gap-3">
          <div>
            <div class="text-xs font-medium mb-1.5">源网卡（需有互联网）</div>
            <Select v-model="inetSrc">
              <SelectTrigger placeholder="选择有网络的网卡" />
              <SelectContent>
                <SelectItem
                  v-for="nic in srcNics"
                  :key="nic.name + nic.ip"
                  :value="nic.name"
                >
                  {{ nic.name }}（{{ nic.ip }}）
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div>
            <div class="text-xs font-medium mb-1.5">目标网口（接设备，支持 USB 转接）</div>
            <Select v-model="inetDst">
              <SelectTrigger placeholder="选择接设备的网口" />
              <SelectContent>
                <SelectItem
                  v-for="nic in dstNics"
                  :key="nic.name + nic.ip"
                  :value="nic.name"
                >
                  {{ nic.name }}（{{
                    !nic.up
                      ? "已断开，可先选择后连接网线"
                      : nic.ip || "未配置 IPv4，共享后由系统自动配置"
                  }}）
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
        <div v-if="dhcp.running" class="text-xs text-muted-foreground mt-3">
          内置 DHCP 正在运行，与网络中继互斥，请先停止 DHCP 再开启
        </div>
      </template>
    </Card>

    <!-- 网络恢复：异常退出残留的一键清理入口 -->
    <Card class="p-5">
      <div class="flex items-center justify-between">
        <div>
          <div class="font-medium text-sm">网络恢复</div>
          <div class="text-xs text-muted-foreground mt-1">
            程序被强制关闭或异常关机后，若残留了共享开关或 192.168.134.1 地址，可一键恢复原状
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          :disabled="restoring || dhcp.running || inetShare.running"
          @click="restoreNet"
        >
          <Loader2 v-if="restoring" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
          {{ restoring ? "恢复中…" : "恢复网络原状" }}
        </Button>
      </div>
      <div
        v-if="dhcp.running || inetShare.running"
        class="text-xs text-muted-foreground mt-2"
      >
        DHCP / 中继运行中无需恢复，停止时会自动还原网卡
      </div>
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
    <!-- 真实网络风险确认 -->
    <ConfirmDialog
      v-model:open="confirmRealNetwork"
      title="检测到真实网络"
      :description="realNetworkMsg"
      confirm-text="仍然启动"
      danger
      @confirm="doStartDhcp(true)"
    />
    <!-- DHCP 网卡选择：独立于首页工作网卡，便于 Wi-Fi 扫描 + 网口 DHCP 并行 -->
    <Dialog v-model:open="confirmNic">
      <DialogContent class="max-w-md" :show-close="false">
        <div class="space-y-2.5">
          <DialogTitle class="text-base">选择 DHCP 网卡</DialogTitle>
          <DialogDescription class="text-sm leading-relaxed">
            与首页工作网卡相互独立：DHCP 仅作用于这里选择的网卡，首页扫描网卡可随时自由切换。
          </DialogDescription>
          <div class="flex items-center gap-2">
            <Select v-model="dhcpNic">
              <SelectTrigger placeholder="选择启用 DHCP 的网卡" />
              <SelectContent>
                <SelectItem
                  v-for="nic in nicList"
                  :key="nic.name + nic.ip"
                  :value="nic.name"
                >
                  {{ nic.name }}（{{
                    !nic.up
                      ? "已断开，可先启用 DHCP 再连接设备"
                      : nic.ip || "未配置 IPv4，开 DHCP 自动配置 134.1"
                  }}）
                </SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              size="icon"
              class="shrink-0"
              :disabled="nicLoading"
              @click="reloadNics"
            >
              <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': nicLoading }" />
            </Button>
          </div>
          <div class="flex justify-end gap-2 pt-1">
            <Button variant="outline" size="sm" @click="confirmNic = false">取消</Button>
            <Button size="sm" :disabled="!dhcpNic" @click="confirmNic = false; confirmNicSelected()">
              启动
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  </div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, EVENTS } from "@/lib/api";
import type { RzjDevice, RzjRelease } from "@/types";
import { useToast } from "@/components/ui/toast/use-toast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Loader2, RefreshCw, Smartphone, X } from "lucide-vue-next";

/**
 * ADB 终端弹窗内的右侧滑出面板：已连接设备列表 + 入住机版本选择安装。
 * 安装任务由后端驱动，面板开关不影响任务；进度经 rzj://progress 事件更新。
 */
const open = defineModel<boolean>("open", { default: false });
const { toast } = useToast();

const devices = ref<RzjDevice[]>([]);
const loading = ref(false);
const listError = ref("");

/** serial -> 进行中任务进度 */
const progress = reactive<
  Record<string, { stage: string; percent: number; message: string }>
>({});

// 版本选择视图
const view = ref<"devices" | "releases">("devices");
const releases = ref<RzjRelease[]>([]);
const releasesLoading = ref(false);
const releasesError = ref("");
const selectedUrl = ref("");
const installingSerial = ref("");

let unlisten: UnlistenFn | null = null;

async function refresh() {
  loading.value = true;
  listError.value = "";
  try {
    devices.value = await api.rzjDevices();
  } catch (e) {
    listError.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function onInstall(device: RzjDevice) {
  installingSerial.value = device.serial;
  view.value = "releases";
  releasesError.value = "";
  selectedUrl.value = "";
  releasesLoading.value = true;
  try {
    releases.value = await api.rzjReleases();
  } catch (e) {
    releasesError.value = String(e);
  } finally {
    releasesLoading.value = false;
  }
}

async function confirmInstall() {
  if (!selectedUrl.value) return;
  try {
    await api.rzjInstall(installingSerial.value, selectedUrl.value);
  } catch (e) {
    toast({ title: "安装发起失败", description: String(e), variant: "destructive" });
  }
  view.value = "devices";
}

function onProgress(p: { serial: string; stage: string; percent: number; message: string }) {
  if (p.stage === "done") {
    progress[p.serial] = { stage: p.stage, percent: p.percent, message: p.message };
    toast({ title: "安装完成", description: p.message, variant: "success" });
    setTimeout(() => {
      delete progress[p.serial];
    }, 2500);
  } else if (p.stage === "error") {
    delete progress[p.serial];
    toast({ title: "安装失败", description: p.message, variant: "destructive" });
  } else {
    progress[p.serial] = { stage: p.stage, percent: p.percent, message: p.message };
  }
}

function stageText(p: { stage: string; percent: number }): string {
  switch (p.stage) {
    case "download":
      return `下载中 ${p.percent}%`;
    case "install":
      return p.percent > 0 ? `安装中 ${p.percent}%` : "安装中…";
    case "launch":
      return "拉起应用…";
    case "done":
      return "已完成";
    default:
      return "处理中…";
  }
}

function statusInfo(d: RzjDevice): {
  label: string;
  variant: "success" | "warning" | "secondary";
} {
  if (d.status === "device") return { label: "已连接", variant: "success" };
  if (d.status === "unauthorized") return { label: "未授权", variant: "warning" };
  return { label: "离线", variant: "secondary" };
}

watch(open, (v) => {
  if (v) refresh();
});

onMounted(async () => {
  unlisten = await listen<{
    serial: string;
    stage: string;
    percent: number;
    message: string;
  }>(EVENTS.RZJ_PROGRESS, (e) => onProgress(e.payload));
});

onBeforeUnmount(() => {
  unlisten?.();
});
</script>

<template>
  <transition
    enter-active-class="transition-transform duration-200"
    enter-from-class="translate-x-full"
    leave-active-class="transition-transform duration-200"
    leave-to-class="translate-x-full"
  >
    <aside
      v-if="open"
      class="absolute inset-y-0 right-0 z-20 flex w-[360px] flex-col border-l bg-background shadow-xl"
    >
      <div class="flex shrink-0 items-center gap-2 border-b px-4 py-3">
        <Smartphone class="h-4 w-4 text-muted-foreground" />
        <span class="text-sm font-semibold">已连接设备</span>
        <div class="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="icon" class="h-7 w-7" :disabled="loading" @click="refresh">
            <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': loading }" />
          </Button>
          <Button variant="ghost" size="icon" class="h-7 w-7" @click="open = false">
            <X class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      <!-- 版本选择视图 -->
      <div v-if="view === 'releases'" class="flex-1 overflow-y-auto p-4">
        <p class="mb-2 text-xs text-muted-foreground">
          为 {{ installingSerial }} 选择要安装的入住机版本
        </p>
        <p v-if="releasesLoading" class="flex items-center gap-2 py-4 text-xs text-muted-foreground">
          <Loader2 class="h-3.5 w-3.5 animate-spin" /> 正在加载版本列表…
        </p>
        <p v-else-if="releasesError" class="py-4 text-xs text-destructive">{{ releasesError }}</p>
        <div v-else class="flex flex-col gap-2">
          <label
            v-for="r in releases"
            :key="r.key"
            class="flex cursor-pointer items-center gap-2 rounded-md border p-2.5 text-sm transition-colors hover:bg-accent"
            :class="{ 'border-primary': selectedUrl === r.url }"
          >
            <input
              v-model="selectedUrl"
              type="radio"
              name="rzj-release"
              :value="r.url"
              class="accent-primary"
            />
            <span class="flex-1">{{ r.name }}</span>
            <span class="text-xs text-muted-foreground">v{{ r.version }}</span>
          </label>
        </div>
        <div class="mt-4 flex justify-end gap-2">
          <Button variant="outline" size="sm" @click="view = 'devices'">返回</Button>
          <Button
            size="sm"
            :disabled="!selectedUrl || releasesLoading || !!releasesError"
            @click="confirmInstall"
          >
            确定
          </Button>
        </div>
      </div>

      <!-- 设备列表视图 -->
      <div v-else class="flex-1 overflow-y-auto p-3">
        <p v-if="listError" class="p-2 text-xs text-destructive">{{ listError }}</p>
        <p
          v-else-if="!loading && devices.length === 0"
          class="p-6 text-center text-xs text-muted-foreground"
        >
          未检测到已连接设备
        </p>
        <ul v-else class="flex flex-col gap-2">
          <li v-for="d in devices" :key="d.serial" class="rounded-md border p-2.5">
            <div class="flex items-center gap-2">
              <span class="min-w-0 flex-1 truncate font-mono text-xs">{{ d.serial }}</span>
              <Badge :variant="statusInfo(d).variant">{{ statusInfo(d).label }}</Badge>
              <Badge variant="outline">{{ d.transport === "tcp" ? "网络" : "USB" }}</Badge>
            </div>
            <div v-if="d.status === 'device'" class="mt-2">
              <!-- 任务进行中：按钮位置替换为进度条 -->
              <div v-if="progress[d.serial]" class="flex flex-col gap-1">
                <div class="flex justify-between text-xs text-muted-foreground">
                  <span>{{ stageText(progress[d.serial]) }}</span>
                </div>
                <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                  <div
                    class="h-full rounded-full transition-all"
                    :class="progress[d.serial].stage === 'done' ? 'bg-success' : 'bg-primary'"
                    :style="{
                      width:
                        progress[d.serial].stage === 'launch' || progress[d.serial].stage === 'done'
                          ? '100%'
                          : progress[d.serial].percent + '%',
                    }"
                  />
                </div>
              </div>
              <Button v-else size="sm" variant="outline" class="w-full" @click="onInstall(d)">
                安装入住机
              </Button>
            </div>
          </li>
        </ul>
      </div>
    </aside>
  </transition>
</template>

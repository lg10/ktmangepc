<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useFileStore } from "@/stores/file";
import { useTaskStore } from "@/stores/task";
import { useUiStore } from "@/stores/ui";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import FetchFileDialog from "@/components/FetchFileDialog.vue";
import DevicePickerDialog from "@/components/DevicePickerDialog.vue";
import type { FileEntry, FileKind } from "@/types";
import {
  CloudDownload,
  Cpu,
  FileCog,
  Trash2,
  Upload,
  FolderArchive,
  RefreshCw,
} from "lucide-vue-next";

const fileStore = useFileStore();
const taskStore = useTaskStore();
const uiStore = useUiStore();
const { toast } = useToast();

const tab = ref<FileKind>("firmware");
const sizeFilter = ref<number>(0); // 0=全部 1024=新版 512=旧版
const fetchOpen = ref(false);
const pickerOpen = ref(false);
const pushing = ref<FileEntry | null>(null);

const list = computed(() => {
  const src = tab.value === "firmware" ? fileStore.firmware : fileStore.config;
  if (!sizeFilter.value) return src;
  return src.filter((f) => f.size === sizeFilter.value);
});

onMounted(async () => {
  await Promise.all([
    fileStore.refresh("firmware").catch(() => {}),
    fileStore.refresh("config").catch(() => {}),
  ]);
});

function switchTab(k: FileKind) {
  tab.value = k;
  sizeFilter.value = 0;
}

async function refresh() {
  await fileStore.refresh(tab.value).catch((e) =>
    toast({ title: "刷新失败", description: String(e), variant: "destructive" })
  );
}

async function remove(f: FileEntry) {
  try {
    await fileStore.remove(tab.value, f.uid);
    toast({ title: "已删除", description: f.name });
  } catch (e) {
    toast({ title: "删除失败", description: String(e), variant: "destructive" });
  }
}

function pushToDevice(f: FileEntry) {
  pushing.value = f;
  pickerOpen.value = true;
}

async function onDevicesPicked(equipIds: string[]) {
  if (!pushing.value) return;
  try {
    const n = await taskStore.start(tab.value, equipIds, pushing.value.uid);
    toast({
      title: tab.value === "firmware" ? "固件升级任务已创建" : "配置下发任务已创建",
      description: `共 ${n} 台设备，等待设备应答…`,
    });
    uiStore.openDock("tasks");
  } catch (e) {
    toast({ title: "创建任务失败", description: String(e), variant: "destructive" });
  } finally {
    pushing.value = null;
  }
}

function fmtTime(s: number) {
  if (!s) return "—";
  return new Date(s * 1000).toLocaleString("zh-CN", { hour12: false });
}
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- 工具栏 -->
    <div class="shrink-0 px-6 pt-4 pb-3 flex items-center gap-3 flex-wrap">
      <div class="flex items-center gap-1 rounded-lg bg-muted p-1">
        <button
          class="px-4 py-1.5 rounded-md text-sm transition-colors flex items-center gap-1.5"
          :class="tab === 'firmware' ? 'bg-background shadow-sm font-medium' : 'text-muted-foreground'"
          @click="switchTab('firmware')"
        >
          <Cpu class="h-3.5 w-3.5" />
          固件文件
        </button>
        <button
          class="px-4 py-1.5 rounded-md text-sm transition-colors flex items-center gap-1.5"
          :class="tab === 'config' ? 'bg-background shadow-sm font-medium' : 'text-muted-foreground'"
          @click="switchTab('config')"
        >
          <FileCog class="h-3.5 w-3.5" />
          配置文件
        </button>
      </div>

      <!-- 固件分包规格筛选 -->
      <div v-if="tab === 'firmware'" class="flex items-center gap-1 text-xs">
        <button
          v-for="opt in [
            { v: 0, label: '全部' },
            { v: 1024, label: '新版 1024B' },
            { v: 512, label: '旧版 512B' },
          ]"
          :key="opt.v"
          class="rounded-md border px-2 py-1 transition-colors"
          :class="sizeFilter === opt.v ? 'border-primary text-primary bg-primary/5' : 'text-muted-foreground hover:bg-accent'"
          @click="sizeFilter = opt.v"
        >
          {{ opt.label }}
        </button>
      </div>

      <div class="ml-auto flex items-center gap-2">
        <Badge variant="outline">{{ list.length }}</Badge>
        <Button variant="ghost" size="sm" @click="refresh">
          <RefreshCw class="h-3.5 w-3.5" />
          刷新
        </Button>
        <Button size="sm" @click="fetchOpen = true">
          <CloudDownload class="h-3.5 w-3.5" />
          云端拉取
        </Button>
      </div>
    </div>

    <!-- 文件表格 -->
    <div class="flex-1 min-h-0 overflow-auto px-6 pb-6">
      <table v-if="list.length" class="w-full text-sm border-collapse">
        <thead class="sticky top-0 bg-background z-10">
          <tr class="text-left text-xs text-muted-foreground border-b">
            <th class="py-2.5 px-3 font-medium">文件名</th>
            <th class="py-2.5 px-3 font-medium w-24">版本</th>
            <th class="py-2.5 px-3 font-medium w-24 hidden min-[1000px]:table-cell">作者</th>
            <th v-if="tab === 'config'" class="py-2.5 px-3 font-medium w-28 hidden min-[1000px]:table-cell">
              房型
            </th>
            <th class="py-2.5 px-3 font-medium w-24 hidden min-[1100px]:table-cell">分包大小</th>
            <th class="py-2.5 px-3 font-medium w-40 hidden min-[1100px]:table-cell">入库时间</th>
            <th class="py-2.5 px-3 font-medium w-36">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="f in list"
            :key="f.uid"
            class="border-b border-border/60 hover:bg-muted/40 transition-colors"
          >
            <td class="py-2.5 px-3">
              <div class="font-medium text-[13px]">
                {{ f.hotelName ? `${f.hotelName}-${f.name}` : f.name }}
              </div>
              <div class="text-[11px] text-muted-foreground font-mono">UID {{ f.uid }}</div>
            </td>
            <td class="py-2.5 px-3 text-xs">{{ f.version }}</td>
            <td class="py-2.5 px-3 text-xs hidden min-[1000px]:table-cell">{{ f.author || "—" }}</td>
            <td v-if="tab === 'config'" class="py-2.5 px-3 text-xs hidden min-[1000px]:table-cell">
              {{ f.roomTypeName || "—" }}
            </td>
            <td class="py-2.5 px-3 text-xs hidden min-[1100px]:table-cell">{{ f.size }} B</td>
            <td class="py-2.5 px-3 text-xs text-muted-foreground hidden min-[1100px]:table-cell">
              {{ fmtTime(f.createTime) }}
            </td>
            <td class="py-2.5 px-3">
              <div class="flex items-center gap-1">
                <Button variant="outline" size="sm" class="h-7 text-xs" @click="pushToDevice(f)">
                  <Upload class="h-3 w-3" />
                  {{ tab === "firmware" ? "升级" : "下发" }}
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7 text-muted-foreground hover:text-destructive"
                  title="删除文件"
                  @click="remove(f)"
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </Button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>

      <div v-else class="mt-24 flex flex-col items-center gap-2 text-muted-foreground">
        <FolderArchive class="h-8 w-8 opacity-40" />
        <p class="text-sm">文件库为空</p>
        <p class="text-xs">点击右上角「云端拉取」，粘贴 download 链接入库</p>
      </div>
    </div>

    <FetchFileDialog v-model:open="fetchOpen" :kind="tab" />
    <DevicePickerDialog
      v-model:open="pickerOpen"
      :title="tab === 'firmware' ? '选择升级设备' : '选择下发设备'"
      :description="pushing ? `文件：${pushing.name}（${pushing.version}）` : ''"
      @confirm="onDevicesPicked"
    />
  </div>
</template>

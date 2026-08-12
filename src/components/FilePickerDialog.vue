<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useFileStore } from "@/stores/file";
import type { FileEntry, FileKind } from "@/types";
import { Loader2, FolderOpen, CloudDownload } from "lucide-vue-next";
import FetchFileDialog from "@/components/FetchFileDialog.vue";

const props = defineProps<{
  open: boolean;
  kind: FileKind;
  title: string;
  description?: string;
}>();
const emit = defineEmits<{
  (e: "update:open", v: boolean): void;
  (e: "confirm", file: FileEntry): void;
}>();

const fileStore = useFileStore();
const picked = ref<FileEntry | null>(null);
const loading = ref(false);
const importOpen = ref(false);
/** 固件分包大小筛选：0=全部 1024=新版 512=旧版（配置文件恒 512 不需筛选） */
const sizeFilter = ref<number>(0);

const list = computed(() => {
  const src = props.kind === "firmware" ? fileStore.firmware : fileStore.config;
  if (!sizeFilter.value) return src;
  return src.filter((f) => f.size === sizeFilter.value);
});

function setSizeFilter(v: number) {
  sizeFilter.value = v;
  // 切换筛选后若已选文件不在列表内，清空选择
  if (picked.value && !list.value.some((f) => f.uid === picked.value!.uid)) {
    picked.value = null;
  }
}

/** 云端导入弹窗关闭：列表已被 fetch action 刷新，自动选中新导入的文件 */
watch(importOpen, (v, prev) => {
  if (!v && prev) void onImported();
});

async function onImported() {
  await fileStore.refresh(props.kind).catch(() => {});
  const newest = list.value[0];
  if (newest) picked.value = newest;
}

watch(
  () => props.open,
  async (open) => {
    if (open) {
      picked.value = null;
      sizeFilter.value = 0;
      loading.value = true;
      try {
        await fileStore.refresh(props.kind);
      } finally {
        loading.value = false;
      }
    }
  }
);

function fmtTime(s: number) {
  if (!s) return "—";
  return new Date(s * 1000).toLocaleString("zh-CN", { hour12: false });
}

function confirm() {
  if (!picked.value) return;
  emit("confirm", picked.value);
  emit("update:open", false);
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogContent class="max-w-2xl">
      <div class="space-y-1">
        <DialogTitle>{{ title }}</DialogTitle>
        <DialogDescription v-if="description">{{ description }}</DialogDescription>
      </div>

      <!-- 固件分包大小筛选 -->
      <div v-if="kind === 'firmware'" class="flex items-center gap-1.5 text-xs">
        <span class="text-muted-foreground">分包大小：</span>
        <button
          v-for="opt in [
            { v: 0, label: '全部' },
            { v: 1024, label: '新版 1024B' },
            { v: 512, label: '旧版 512B' },
          ]"
          :key="opt.v"
          class="rounded-md border px-2 py-1 transition-colors"
          :class="sizeFilter === opt.v ? 'border-primary text-primary bg-primary/5' : 'text-muted-foreground hover:bg-accent'"
          @click="setSizeFilter(opt.v)"
        >
          {{ opt.label }}
        </button>
      </div>

      <div class="h-80 overflow-auto rounded-md border">
        <div v-if="loading" class="h-full flex items-center justify-center text-muted-foreground">
          <Loader2 class="h-5 w-5 animate-spin" />
        </div>
        <template v-else-if="list.length">
          <table class="w-full text-xs border-collapse">
            <thead class="sticky top-0 bg-muted/80 backdrop-blur z-10">
              <tr class="text-left text-muted-foreground">
                <th class="py-2 px-3 font-medium">文件</th>
                <th class="py-2 px-3 font-medium w-20">版本</th>
                <th class="py-2 px-3 font-medium w-20 hidden min-[700px]:table-cell">作者</th>
                <th class="py-2 px-3 font-medium w-24">分包大小</th>
                <th class="py-2 px-3 font-medium w-36 hidden min-[700px]:table-cell">入库时间</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="f in list"
                :key="f.uid"
                class="border-t border-border/60 cursor-pointer hover:bg-muted/50"
                :class="picked?.uid === f.uid && 'bg-primary/10'"
                @dblclick="picked = f; confirm()"
                @click="picked = f"
              >
                <td class="py-2 px-3">
                  <div class="font-medium truncate max-w-64">
                    {{ f.hotelName ? `${f.hotelName}-${f.name}` : f.name }}
                  </div>
                  <div v-if="kind === 'config' && f.roomTypeName" class="text-muted-foreground">
                    {{ f.roomTypeName }}
                  </div>
                </td>
                <td class="py-2 px-3">{{ f.version }}</td>
                <td class="py-2 px-3 hidden min-[700px]:table-cell">{{ f.author || "—" }}</td>
                <td class="py-2 px-3">
                  {{ f.size }}B{{ kind === 'firmware' ? (f.size === 1024 ? ' · 新版' : ' · 旧版') : '' }}
                </td>
                <td class="py-2 px-3 text-muted-foreground hidden min-[700px]:table-cell">
                  {{ fmtTime(f.createTime) }}
                </td>
              </tr>
            </tbody>
          </table>
        </template>
        <div v-else class="h-full flex flex-col items-center justify-center gap-2 text-muted-foreground">
          <FolderOpen class="h-6 w-6 opacity-40" />
          <p class="text-xs">
            {{ fileStore.firmware.length || fileStore.config.length ? '当前筛选下无文件，试试切换分包大小或选「全部」' : '文件库为空，点击左下「导入」从云端拉取文件' }}
          </p>
        </div>
      </div>

      <div class="flex items-center justify-between gap-2 pt-1">
        <Button variant="outline" size="sm" @click="importOpen = true">
          <CloudDownload class="h-3.5 w-3.5" />
          导入
        </Button>
        <div class="flex items-center gap-2">
          <Button variant="outline" @click="emit('update:open', false)">取消</Button>
          <Button :disabled="!picked" @click="confirm">
            确定{{ picked ? `（${picked.version}）` : "" }}
          </Button>
        </div>
      </div>
    </DialogContent>

    <!-- 云端导入：粘贴 download 链接直接入文件库 -->
    <FetchFileDialog v-model:open="importOpen" :kind="kind" />
  </Dialog>
</template>

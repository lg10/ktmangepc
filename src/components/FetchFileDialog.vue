<script setup lang="ts">
import { ref, watch } from "vue";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFileStore } from "@/stores/file";
import { useToast } from "@/components/ui/toast/use-toast";
import type { FileKind } from "@/types";
import { Loader2, CloudDownload } from "lucide-vue-next";

const props = defineProps<{
  open: boolean;
  kind: FileKind;
}>();
const emit = defineEmits<{
  (e: "update:open", v: boolean): void;
}>();

const fileStore = useFileStore();
const { toast } = useToast();
const url = ref("");

watch(
  () => props.open,
  (open) => {
    if (open) url.value = "";
  }
);

async function start() {
  const link = url.value.trim();
  if (!link) {
    toast({ title: "请输入云端下载链接", variant: "destructive" });
    return;
  }
  try {
    const entry = await fileStore.fetch(props.kind, link);
    toast({ title: "文件已载入文件库", description: `${entry.name} · ${entry.version}` });
    emit("update:open", false);
  } catch (e) {
    toast({ title: "拉取失败", description: String(e), variant: "destructive" });
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="!fileStore.fetching && emit('update:open', $event)">
    <DialogContent class="max-w-lg">
      <div class="space-y-1">
        <DialogTitle>从云端拉取{{ kind === "firmware" ? "固件" : "配置" }}文件</DialogTitle>
        <DialogDescription>
          粘贴云端 download 链接，自动获取文件信息、下载并按分包规则入库
        </DialogDescription>
      </div>

      <div class="space-y-3">
        <Input
          v-model="url"
          :disabled="fileStore.fetching"
          placeholder="https://…/download/…"
          @keydown.enter="!fileStore.fetching && start()"
        />

        <!-- 进度区 -->
        <div
          v-if="fileStore.fetching"
          class="rounded-md border bg-muted/40 px-4 py-3 space-y-2"
        >
          <div class="flex items-center gap-2 text-xs text-foreground/80">
            <Loader2 class="h-3.5 w-3.5 animate-spin text-primary" />
            {{ fileStore.progress.message || "准备中…" }}
          </div>
          <div class="h-1.5 rounded-full bg-muted overflow-hidden">
            <div
              class="h-full rounded-full bg-primary transition-all duration-300"
              :style="{ width: fileStore.progress.percent + '%' }"
            />
          </div>
        </div>
      </div>

      <div class="flex items-center justify-end gap-2 pt-1">
        <Button variant="outline" :disabled="fileStore.fetching" @click="emit('update:open', false)">
          取消
        </Button>
        <Button :disabled="fileStore.fetching" @click="start">
          <CloudDownload v-if="!fileStore.fetching" class="h-4 w-4" />
          <Loader2 v-else class="h-4 w-4 animate-spin" />
          开始拉取
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>

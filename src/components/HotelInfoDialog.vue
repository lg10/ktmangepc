<script setup lang="ts">
import { ref } from "vue";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useHotelStore } from "@/stores/hotel";
import { useToast } from "@/components/ui/toast/use-toast";
import { Loader2, RefreshCw } from "lucide-vue-next";

defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "update:open", v: boolean): void }>();

const hotelStore = useHotelStore();
const { toast } = useToast();
const updating = ref(false);

function fmtDeadline(s: number) {
  if (!s) return "—";
  return new Date(s * 1000).toLocaleString("zh-CN", { hour12: false });
}

/** 更新信息：重新拉取酒店房间数据与授权信息 */
async function updateInfo() {
  if (updating.value || hotelStore.hotelId <= 0) return;
  updating.value = true;
  try {
    const n = await hotelStore.sync();
    await hotelStore.fetchAuthInfo().catch(() => {});
    toast({
      title: "信息已更新",
      description: `${hotelStore.hotelName} · ${n} 个房间`,
    });
  } catch (e) {
    toast({ title: "更新失败", description: String(e), variant: "destructive" });
  } finally {
    updating.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogContent class="max-w-md">
      <div class="space-y-1">
        <DialogTitle>酒店详情</DialogTitle>
        <DialogDescription>当前酒店的云端信息与授权状态</DialogDescription>
      </div>

      <div class="rounded-md border divide-y text-sm">
        <div class="flex items-center justify-between px-4 py-2.5">
          <span class="text-muted-foreground">酒店 ID</span>
          <span class="font-mono">{{ hotelStore.hotelId || "—" }}</span>
        </div>
        <div class="flex items-center justify-between px-4 py-2.5">
          <span class="text-muted-foreground">酒店名称</span>
          <span class="font-medium">{{ hotelStore.hotelName || "未选择酒店" }}</span>
        </div>
        <div class="flex items-center justify-between px-4 py-2.5">
          <span class="text-muted-foreground">已同步房间</span>
          <span>{{ hotelStore.state.roomCount }} 个</span>
        </div>
        <div class="flex items-center justify-between px-4 py-2.5">
          <span class="text-muted-foreground">授权状态</span>
          <span class="font-medium" :class="hotelStore.authInfo ? 'text-primary' : 'text-muted-foreground'">
            {{ hotelStore.authInfo?.text || "未获取，点击下方「更新信息」拉取" }}
          </span>
        </div>
        <div v-if="hotelStore.authInfo" class="flex items-center justify-between px-4 py-2.5">
          <span class="text-muted-foreground">授权类型</span>
          <span>{{ hotelStore.authInfo.permanent ? "永久授权" : "限期授权" }}</span>
        </div>
        <div v-if="hotelStore.authInfo && !hotelStore.authInfo.permanent" class="flex items-center justify-between px-4 py-2.5">
          <span class="text-muted-foreground">截止时间</span>
          <span>{{ fmtDeadline(hotelStore.authInfo.deadline) }}</span>
        </div>
      </div>

      <div class="flex items-center justify-end gap-2 pt-1">
        <Button variant="outline" :disabled="updating" @click="emit('update:open', false)">关闭</Button>
        <Button :disabled="updating || hotelStore.hotelId <= 0" @click="updateInfo">
          <RefreshCw v-if="!updating" class="h-4 w-4" />
          <Loader2 v-else class="h-4 w-4 animate-spin" />
          更新信息
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>

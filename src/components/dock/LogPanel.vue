<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { useDeviceStore } from "@/stores/device";
import { Eraser } from "lucide-vue-next";

const deviceStore = useDeviceStore();
const listRef = ref<HTMLElement | null>(null);
const follow = ref(true);

watch(
  () => deviceStore.logs.length,
  async () => {
    if (!follow.value) return;
    await nextTick();
    listRef.value?.scrollTo({ top: listRef.value.scrollHeight });
  }
);

function onScroll() {
  const el = listRef.value;
  if (!el) return;
  follow.value = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

function clear() {
  deviceStore.logs = [];
}
</script>

<template>
  <div class="h-full relative">
    <div
      ref="listRef"
      class="h-full overflow-auto px-3 py-2 font-mono text-[11px] leading-5 select-text"
      @scroll="onScroll"
    >
      <div v-if="deviceStore.logs.length === 0" class="text-muted-foreground/60">
        暂无日志
      </div>
      <div
        v-for="(line, i) in deviceStore.logs"
        :key="i"
        class="whitespace-pre-wrap break-all text-foreground/80"
      >
        {{ line }}
      </div>
    </div>
    <button
      class="absolute right-2 top-2 flex items-center gap-1 rounded border bg-background/80 px-1.5 py-0.5 text-[10px] text-muted-foreground hover:text-foreground"
      title="清空日志"
      @click="clear"
    >
      <Eraser class="h-3 w-3" />
      清空
    </button>
  </div>
</template>

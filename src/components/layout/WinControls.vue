<script setup lang="ts">
/**
 * Windows/Linux 平台自绘窗口控件（微信 Windows 版风格）：
 * 右对齐 最小化 / 最大化-还原 / 关闭，关闭悬停红色高亮。
 * macOS 不使用本组件（原生红绿灯）。
 */
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-vue-next";

withDefaults(defineProps<{ /** 登录窗等不可最大化场景隐藏最大化按钮 */ maximize?: boolean }>(), {
  maximize: true,
});

const win = getCurrentWindow();
const isMax = ref(false);
let unlisten: (() => void) | null = null;

onMounted(async () => {
  isMax.value = await win.isMaximized();
  unlisten = await win.onResized(async () => {
    isMax.value = await win.isMaximized();
  });
});
onUnmounted(() => unlisten?.());
</script>

<template>
  <div class="flex h-full shrink-0">
    <button
      class="w-11 h-full flex items-center justify-center text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
      title="最小化"
      @click="win.minimize()"
    >
      <Minus class="h-3.5 w-3.5" />
    </button>
    <button
      v-if="maximize"
      class="w-11 h-full flex items-center justify-center text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
      title="最大化 / 还原"
      @click="win.toggleMaximize()"
    >
      <Copy v-if="isMax" class="h-3 w-3" />
      <Square v-else class="h-3 w-3" />
    </button>
    <button
      class="w-11 h-full flex items-center justify-center text-muted-foreground hover:bg-[#e81123] hover:text-white transition-colors"
      title="关闭"
      @click="win.close()"
    >
      <X class="h-3.5 w-3.5" />
    </button>
  </div>
</template>

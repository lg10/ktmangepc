<script setup lang="ts">
/**
 * Windows/Linux 平台自绘窗口控件，按微软官方标题栏规范（learn.microsoft.com/windows/apps/design/basics/titlebar-design）：
 * - 控件按钮 46px 宽、通高（full-bleed）；图标 10px 细线（仿 Fluent ChromeMinimize/Maximize/Close）
 * - 最小化/最大化 hover 为弱填充，关闭按钮 hover/pressed 为系统红（Win11 #C42B1C）
 * - 双击标题栏最大化/还原、拖动等规范行为由 TitleBar 层实现
 * macOS 不使用本组件（原生红绿灯，12pt 圆/8pt 间距由系统渲染）。
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
      class="w-[46px] h-full flex items-center justify-center text-muted-foreground hover:bg-muted hover:text-foreground active:bg-muted/50 transition-colors"
      title="最小化"
      @click="win.minimize()"
    >
      <Minus class="h-2.5 w-2.5" :stroke-width="1.5" />
    </button>
    <button
      v-if="maximize"
      class="w-[46px] h-full flex items-center justify-center text-muted-foreground hover:bg-muted hover:text-foreground active:bg-muted/50 transition-colors"
      title="最大化 / 还原"
      @click="win.toggleMaximize()"
    >
      <Copy v-if="isMax" class="h-2.5 w-2.5" :stroke-width="1.5" />
      <Square v-else class="h-2.5 w-2.5" :stroke-width="1.5" />
    </button>
    <button
      class="w-[46px] h-full flex items-center justify-center text-muted-foreground hover:bg-[#c42b1c] hover:text-white active:bg-[#b31c15] active:text-white transition-colors"
      title="关闭"
      @click="win.close()"
    >
      <X class="h-2.5 w-2.5" :stroke-width="1.5" />
    </button>
  </div>
</template>

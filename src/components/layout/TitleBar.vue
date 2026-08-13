<script setup lang="ts">
import { computed, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Building2, Command, Info } from "lucide-vue-next";
import { useUiStore } from "@/stores/ui";
import { useHotelStore } from "@/stores/hotel";
import { isMac } from "@/lib/window";
import WinControls from "@/components/layout/WinControls.vue";
import HotelInfoDialog from "@/components/HotelInfoDialog.vue";

const uiStore = useUiStore();
const hotelStore = useHotelStore();
const win = getCurrentWindow();

const props = withDefaults(
  defineProps<{
    /** 登录/闪屏等场景隐藏命令面板入口 */
    showPalette?: boolean;
  }>(),
  { showPalette: true }
);

/** 顶栏展示当前酒店名称（替代旧面包屑） */
const hotelLabel = computed(() => {
  if (hotelStore.hotelName) return hotelStore.hotelName;
  if (hotelStore.hotelId > 0) return `酒店 ${hotelStore.hotelId}`;
  return "未选择酒店";
});

/** 酒店详情弹窗：单一 boolean 控制，重复点击不会重复开窗 */
const infoOpen = ref(false);

function openPalette() {
  uiStore.paletteOpen = true;
}

/** 无边框窗口拖动：macOS WKWebView 不认 -webkit-app-region，需手动 startDragging */
function onTitleDrag(e: MouseEvent) {
  if ((e.target as HTMLElement).closest("button")) return;
  win.startDragging();
}
</script>

<template>
  <header
    class="h-9 shrink-0 flex items-center border-b bg-muted/40 titlebar-drag select-none"
    @dblclick="isMac ? undefined : win.toggleMaximize()"
    @mousedown="onTitleDrag"
  >
    <!-- macOS 原生红绿灯占位；Windows/Linux 控件在右侧（微信式平台适配） -->
    <div v-if="isMac" class="w-[70px] shrink-0" />

    <!-- 酒店名称 + 详情按钮（替代旧面包屑） -->
    <div class="flex items-center gap-2 min-w-0" :class="isMac ? 'pl-1' : 'pl-3'">
      <Building2 class="h-3.5 w-3.5 text-primary shrink-0" />
      <span class="text-xs font-semibold text-foreground/90 truncate">{{ hotelLabel }}</span>
      <button
        class="titlebar-no-drag flex items-center gap-1 h-5 rounded-md px-1.5 text-[10px] text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
        title="酒店详情（ID / 名称 / 授权）"
        @click="infoOpen = true"
      >
        <Info class="h-3 w-3" />
        详情
      </button>
    </div>

    <!-- 右侧：命令面板入口 + Windows/Linux 窗口控件 -->
    <div class="ml-auto flex items-stretch h-full titlebar-no-drag">
      <div v-if="props.showPalette" class="flex items-center pr-3">
        <button
          class="flex items-center gap-1.5 rounded-md border bg-background/60 px-2.5 py-1 text-[11px] text-muted-foreground hover:text-foreground hover:bg-background transition-colors"
          title="命令面板（⌘K）"
          @click="openPalette"
        >
          <Command class="h-3 w-3" />
          <span class="hidden min-[1150px]:inline">命令面板</span>
          <kbd class="rounded border bg-muted px-1 text-[10px]">⌘K</kbd>
        </button>
      </div>
      <WinControls v-if="!isMac" />
    </div>
  </header>

  <!-- 酒店详情弹窗 -->
  <HotelInfoDialog v-model:open="infoOpen" />
</template>

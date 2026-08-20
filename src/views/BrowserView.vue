<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import {
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window";
import { Webview } from "@tauri-apps/api/webview";
import { api } from "@/lib/api";
import { isMac } from "@/lib/window";
import { Globe } from "lucide-vue-next";
import WinControls from "@/components/layout/WinControls.vue";

/**
 * 入住机管理台内嵌窗口内容页：
 * - 自绘顶栏（微信式）：地球图标一键转系统浏览器，右侧窗口控制
 * - 内容区为同窗口子 webview（Tauri 2 多 webview），加载设备管理页 http://<ip>:7271/admin/
 */
const route = useRoute();
const url = (route.query.url as string) ?? "";
const title = (route.query.title as string) ?? "入住机管理台";

const win = getCurrentWindow();
/** 顶栏高度（逻辑像素），子 webview 自其下方铺满 */
const BAR_H = 40;
const loadError = ref("");
let content: Webview | null = null;
let unlistenResize: (() => void) | null = null;

/** 按窗口当前尺寸重排子 webview（innerSize 为物理像素，需按缩放比换算） */
async function layout() {
  if (!content) return;
  try {
    const size = await win.innerSize();
    const factor = await win.scaleFactor();
    await content.setPosition(new LogicalPosition(0, BAR_H));
    await content.setSize(
      new LogicalSize(size.width / factor, Math.max(size.height / factor - BAR_H, 1))
    );
  } catch {
    /* 窗口销毁过程中的竞态忽略 */
  }
}

onMounted(async () => {
  if (!url) return;
  content = new Webview(win, `${win.label}-content`, {
    url,
    x: 0,
    y: BAR_H,
    width: 960,
    height: 600,
  });
  // 必须等子 webview 真正创建完成后再调 setPosition/setSize，
  // 否则指令因“webview not found”静默失败导致几何异常（白屏/零尺寸）
  const created = await new Promise<boolean>((resolve) => {
    void content!.once("tauri://created", () => resolve(true));
    void content!.once("tauri://error", (e) => {
      loadError.value = `管理页窗口创建失败：${String(e.payload ?? e)}`;
      resolve(false);
    });
  });
  if (!created) return;
  await layout();
  // 创建后个别平台首帧几何未生效，延迟再同步一次
  setTimeout(() => layout(), 300);
  unlistenResize = await win.onResized(() => layout());
});

onBeforeUnmount(() => {
  unlistenResize?.();
  // 关窗时子 webview 随之销毁，这里主动 close 兜底
  content?.close().catch(() => {});
  content = null;
});

/** 地球图标：用系统默认浏览器打开当前管理台地址 */
function openInBrowser() {
  api.openUrl(url).catch(() => {});
}
</script>

<template>
  <div class="h-screen w-screen flex flex-col overflow-hidden bg-background">
    <!-- 自绘顶栏：可拖动区域 -->
    <header
      data-tauri-drag-region
      class="shrink-0 h-10 border-b bg-muted/30 flex items-center gap-2 px-3 select-none"
    >
    <!-- macOS 红绿灯占位（overlay 标题栏按钮叠在内容之上） -->
      <div v-if="isMac" class="w-[76px] shrink-0" />
      <button
        class="h-7 px-2 flex items-center gap-1.5 rounded-md text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
        title="用系统浏览器打开"
        @click="openInBrowser"
      >
        <Globe class="h-3.5 w-3.5" />
        浏览器打开
      </button>
      <span
        data-tauri-drag-region
        class="flex-1 text-xs text-muted-foreground truncate"
      >
        {{ title }}
      </span>
      <WinControls v-if="!isMac" />
    </header>
    <!-- 顶栏以下区域由子 webview 覆盖渲染设备管理台 -->
    <div class="flex-1 min-h-0 relative">
      <div
        v-if="loadError"
        class="absolute inset-0 flex items-center justify-center text-xs text-destructive px-6 text-center"
      >
        {{ loadError }}
      </div>
    </div>
  </div>
</template>

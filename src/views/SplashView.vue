<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import { useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, EVENTS } from "@/lib/api";
import { setSizeLogin, setSizeSplash, openWorkbenchWindow } from "@/lib/window";
import {
  splashFinish,
  splashLog,
  splashProgress,
  splashTask,
  splashVersion,
} from "@/lib/splash";
import { useAuthStore } from "@/stores/auth";

const router = useRouter();
const authStore = useAuthStore();
let unlisten: UnlistenFn | null = null;

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

onMounted(async () => {
  await setSizeSplash();

  // 闪屏全程保持小窗尺寸：先在小窗淡出，再放大窗口进业务页，避免“跳大页”观感
  splashTask("执行启动检查");
  splashLog("界面已挂载，执行环境检查…");
  splashProgress(38);

  // 后端 check_environment 逐步推送真实日志，就地更新原生闪屏
  unlisten = await listen<{ task?: string; log?: string; progress?: number }>(
    EVENTS.SPLASH_LOG,
    (e) => {
      const p = e.payload;
      if (p.task) splashTask(p.task);
      if (p.log) splashLog(p.log);
      if (typeof p.progress === "number") splashProgress(p.progress);
    }
  );

  try {
    const report = await api.checkEnvironment();
    splashVersion(report.appVersion);

    if (report.loginValid) {
      splashTask("恢复登录状态");
      splashLog("正在从本地凭证恢复会话…");
      const ok = await authStore.restore();
      splashProgress(100);
      splashLog(ok ? `欢迎回来，${authStore.nickName || "用户"}` : "登录凭证已失效，请重新扫码");
      await delay(450);
      splashFinish();
      await delay(320);
      if (ok) {
        // 微信式切窗：新开工作台窗口后关闭本窗
        await openWorkbenchWindow();
        await delay(300);
        getCurrentWindow().close();
      } else {
        await setSizeLogin();
        router.push({ name: "login" });
      }
    } else {
      splashProgress(100);
      splashLog("环境检查完成，跳转扫码登录…");
      await delay(450);
      splashFinish();
      await delay(320);
      await setSizeLogin();
      router.push({ name: "login" });
    }
  } catch (e) {
    console.error(e);
    splashProgress(100);
    splashLog("启动检查异常，跳转登录页");
    splashFinish();
    await delay(320);
    await setSizeLogin();
    router.push({ name: "login" });
  }

  unlisten?.();
});

onBeforeUnmount(() => unlisten?.());
</script>

<template>
  <!-- 闪屏为 #app 之外的原生 DOM，由本页就地更新；此处仅占位 -->
  <div class="h-screen w-screen" />
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import QRCode from "qrcode";
import { api, EVENTS } from "@/lib/api";
import { setSizeLogin, isMac } from "@/lib/window";
import { splashFinish } from "@/lib/splash";
import { useToast } from "@/components/ui/toast/use-toast";
import { Loader2 } from "lucide-vue-next";
import WinControls from "@/components/layout/WinControls.vue";

const { t } = useI18n();
const { toast } = useToast();
const win = getCurrentWindow();

/** 微信式简洁顶栏：仅拖拽区 + 平台窗口控件 */
function onDrag(e: MouseEvent) {
  if ((e.target as HTMLElement).closest("button")) return;
  win.startDragging();
}

type Phase = "loading" | "ready" | "wait" | "refuse" | "error" | "success";
const phase = ref<Phase>("loading");
const qrUrl = ref("");
let unlisteners: UnlistenFn[] = [];
/** 防止并发刷新 */
let seq = 0;

function randomKey(len = 12) {
  const chars = "abcdefghijklmnopqrstuvwxyz0123456789";
  let s = "";
  for (let i = 0; i < len; i++) {
    s += chars[Math.floor(Math.random() * chars.length)];
  }
  return s;
}

async function startLogin() {
  const my = ++seq;
  phase.value = "loading";
  try {
    const code = randomKey();
    // 与原 H5 一致：二维码内容 substring(6,18) 作为轮询 key
    const qrText = await api.buildLoginCode(code);
    if (my !== seq) return;
    const watchKey = qrText.length >= 18 ? qrText.substring(6, 18) : qrText;
    qrUrl.value = await QRCode.toDataURL(qrText, {
      width: 560,
      margin: 2,
      color: { dark: "#000000", light: "#ffffff" },
    });
    if (my !== seq) return;
    phase.value = "ready";
    await api.cancelQrLogin().catch(() => {});
    await api.beginQrLogin(watchKey);
  } catch (e) {
    if (my !== seq) return;
    phase.value = "error";
    toast({ title: t("login.failed"), description: String(e), variant: "destructive" });
  }
}

onMounted(async () => {
  await setSizeLogin();
  // 退出登录后重建的本窗会带原生闪屏层，此处淡出移除（首启流程中已移除则为 no-op）
  splashFinish();
  unlisteners = await Promise.all([
    listen<string>(EVENTS.QR_STATE, (e) => {
      if (e.payload === "wait") phase.value = "wait";
      else if (e.payload === "refuse") phase.value = "refuse";
      else phase.value = "error";
    }),
    listen(EVENTS.LOGIN_SUCCESS, async () => {
      phase.value = "success";
      // 窗口切换由 App.vue 全局监听处理（新开工作台窗 + 关闭本窗）
    }),
    listen<{ message: string }>(EVENTS.LOGIN_FAILED, (e) => {
      phase.value = "error";
      toast({
        title: t("common.failed"),
        description: e.payload.message,
        variant: "destructive",
      });
    }),
  ]);
  await startLogin();
});

onUnmounted(async () => {
  seq++;
  unlisteners.forEach((u) => u());
  await api.cancelQrLogin().catch(() => {});
});
</script>

<template>
  <div class="h-screen w-screen flex flex-col bg-background">
    <!-- 顶栏：macOS 原生红绿灯；Windows/Linux 右侧自绘控件（规范高 32px） -->
    <header class="shrink-0 flex items-center select-none" :class="isMac ? 'h-10' : 'h-8'" @mousedown="onDrag">
      <div v-if="!isMac" class="ml-auto h-full" @mousedown.stop>
        <WinControls :maximize="false" />
      </div>
    </header>

    <div class="flex-1 flex flex-col items-center justify-center select-none">
      <!-- 品牌字标 -->
      <h1 class="text-[22px] font-semibold tracking-[5px] text-foreground">
        {{ t("app.name") }}
      </h1>
      <p class="mt-2 text-[11px] tracking-[2px] text-muted-foreground">{{ t("app.slogan") }}</p>

      <!-- 二维码卡片 -->
      <div
        class="relative mt-9 rounded-2xl border bg-card p-3.5 shadow-sm"
        :class="phase === 'error' || phase === 'refuse' ? 'cursor-pointer' : ''"
        @click="phase === 'error' || phase === 'refuse' ? startLogin() : undefined"
      >
        <div class="h-[236px] w-[236px] rounded-lg overflow-hidden bg-white">
          <img v-if="qrUrl" :src="qrUrl" class="h-full w-full" alt="login qrcode" />
        </div>

        <!-- 状态遮罩 -->
        <div
          v-if="phase === 'loading'"
          class="absolute inset-0 rounded-2xl backdrop-blur-[3px] bg-card/85 flex flex-col items-center justify-center gap-2.5"
        >
          <Loader2 class="h-5 w-5 animate-spin text-muted-foreground" />
          <span class="text-xs text-muted-foreground">{{ t("login.generating") }}</span>
        </div>
        <div
          v-else-if="phase === 'wait'"
          class="absolute inset-0 rounded-2xl backdrop-blur-[3px] bg-card/85 flex items-center justify-center px-6"
        >
          <span class="text-sm font-medium text-success text-center">{{ t("login.wait") }}</span>
        </div>
        <div
          v-else-if="phase === 'refuse'"
          class="absolute inset-0 rounded-2xl backdrop-blur-[3px] bg-card/85 flex flex-col items-center justify-center gap-1.5 px-6"
        >
          <span class="text-sm font-medium text-destructive text-center">
            {{ t("login.refuse") }}
          </span>
          <span class="text-[11px] text-muted-foreground">{{ t("login.refresh") }}</span>
        </div>
        <div
          v-else-if="phase === 'error'"
          class="absolute inset-0 rounded-2xl backdrop-blur-[3px] bg-card/85 flex flex-col items-center justify-center gap-1.5 px-6"
        >
          <span class="text-sm font-medium text-warning text-center">
            {{ t("login.timeout") }}
          </span>
          <span class="text-[11px] text-muted-foreground">{{ t("login.refresh") }}</span>
        </div>
        <div
          v-else-if="phase === 'success'"
          class="absolute inset-0 rounded-2xl backdrop-blur-[3px] bg-card/85 flex items-center justify-center"
        >
          <span class="text-sm font-medium text-success">{{ t("login.success") }}</span>
        </div>
      </div>

      <p class="mt-6 text-xs text-muted-foreground">{{ t("login.scanHint") }}</p>
    </div>

    <p class="pb-4 text-center text-[11px] text-muted-foreground/60 select-none">
      KinginT · 肯天科技集团
    </p>
  </div>
</template>

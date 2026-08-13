<script setup lang="ts">
// 进入工作台前的欢迎语动画：淡入 → 停留 → 淡出（覆盖登录进入与静默恢复两种路径）
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useAuthStore } from "@/stores/auth";

const authStore = useAuthStore();

const phase = ref<"in" | "stay" | "out" | "done">("in");
let timers: ReturnType<typeof setTimeout>[] = [];

onMounted(() => {
  // 下一帧触发过渡，保证淡入动画生效
  timers.push(setTimeout(() => (phase.value = "stay"), 50));
  timers.push(setTimeout(() => (phase.value = "out"), 1600));
  timers.push(setTimeout(() => (phase.value = "done"), 2100));
});

onBeforeUnmount(() => timers.forEach(clearTimeout));
</script>

<template>
  <div
    v-if="phase !== 'done'"
    class="fixed inset-0 z-[100] flex flex-col items-center justify-center bg-background select-none"
    :class="phase === 'in' ? 'opacity-0' : phase === 'out' ? 'opacity-0' : 'opacity-100'"
    :style="{ transition: phase === 'in' ? 'none' : 'opacity 0.45s ease' }"
  >
    <p
      class="text-2xl font-semibold tracking-[3px] text-foreground"
      :class="phase === 'stay' ? 'animate-welcome-rise' : 'opacity-0 translate-y-3'"
    >
      欢迎回来，{{ authStore.nickName || "用户" }}
    </p>
    <p
      class="mt-3 text-xs tracking-[4px] text-muted-foreground"
      :class="phase === 'stay' ? 'animate-welcome-rise-delay' : 'opacity-0 translate-y-3'"
    >
      KT Device Scan · 酒店智能设备管理工具
    </p>
  </div>
</template>

<style scoped>
@keyframes welcome-rise {
  from {
    opacity: 0;
    transform: translateY(12px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
.animate-welcome-rise {
  animation: welcome-rise 0.55s ease both;
}
.animate-welcome-rise-delay {
  animation: welcome-rise 0.55s ease 0.18s both;
}
</style>

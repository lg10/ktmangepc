<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { Loader2, Search, ChevronLeft, ChevronRight } from "lucide-vue-next";
import { useAuthStore } from "@/stores/auth";
import { useHotelStore } from "@/stores/hotel";
import { api } from "@/lib/api";
import { setSizeLogin } from "@/lib/window";
import { splashFinish } from "@/lib/splash";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { HotelSearchRecord } from "@/types";

const PAGE_SIZE = 5;

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const hotelStore = useHotelStore();
const { toast } = useToast();

const phase = ref<"loading" | "ready">("loading");
const keyword = ref("");
const records = ref<HotelSearchRecord[]>([]);
const total = ref(0);
const pageNum = ref(1);
const selected = ref<HotelSearchRecord | null>(null);
const searching = ref(false);
const entering = ref(false);

const pages = computed(() => Math.max(1, Math.ceil(total.value / PAGE_SIZE)));

async function fetchPage(page: number, kw: string) {
  searching.value = true;
  try {
    const res = await api.searchHotel(kw.trim(), page, PAGE_SIZE);
    records.value = res.records;
    total.value = res.total;
    pageNum.value = res.current || page;
  } finally {
    searching.value = false;
  }
}

async function doSearch() {
  await fetchPage(1, keyword.value);
}
async function prevPage() {
  if (pageNum.value > 1) await fetchPage(pageNum.value - 1, keyword.value);
}
async function nextPage() {
  if (pageNum.value < pages.value) await fetchPage(pageNum.value + 1, keyword.value);
}

/** 选中酒店后点击登录：拉取房间列表 + 授权信息，再进工作台 */
async function enter() {
  if (!selected.value || entering.value) return;
  entering.value = true;
  try {
    const n = await hotelStore.enter(selected.value.id, selected.value.name);
    toast({ title: "酒店同步成功", description: `${selected.value.name} · ${n} 个房间` });
    router.replace({ name: "launch" });
  } catch (e) {
    toast({ title: "进入酒店失败", description: String(e), variant: "destructive" });
  } finally {
    entering.value = false;
  }
}

onMounted(async () => {
  // 工作台新窗同样带 index.html 原生闪屏层，立即淡出移除
  splashFinish();
  const inTauri = "__TAURI_INTERNALS__" in window;
  const ok = await authStore.restore();
  if (!ok) {
    if (inTauri) {
      // 登录态失效：本窗直接变为登录窗（不开新窗/关旧窗），
      // 避免与登录窗关闭倒计时竞态导致所有窗口被关闭而闪退
      await setSizeLogin();
      router.replace({ name: "login" });
      return;
    }
    // 非 Tauri 环境（浏览器调试）：继续渲染选择页
  } else if (route.query.fresh !== "1") {
    // 静默恢复启动且本地已有同步缓存：直达工作台，无需重复选择
    try {
      await hotelStore.loadState();
      if (hotelStore.state.synced) {
        router.replace({ name: "launch" });
        return;
      }
    } catch {
      /* 忽略，继续选择流程 */
    }
  } else {
    // 显式扫码登录：必经选择页，仅预加载缓存状态
    await hotelStore.loadState().catch(() => {});
  }
  try {
    await fetchPage(1, "");
  } catch (e) {
    toast({ title: "酒店列表加载失败", description: String(e), variant: "destructive" });
    records.value = [];
  }
  phase.value = "ready";
});
</script>

<template>
  <div class="h-screen w-screen flex items-center justify-center bg-background select-none">
    <div class="w-[400px]">
      <p class="text-center text-[13px] tracking-[2px] text-muted-foreground">
        欢迎回来，{{ authStore.nickName || "用户" }}
      </p>

      <!-- loading：欢迎语后的加载态 -->
      <div v-if="phase === 'loading'" class="mt-14 flex flex-col items-center gap-3">
        <Loader2 class="h-6 w-6 animate-spin text-muted-foreground" />
        <p class="text-xs text-muted-foreground">正在加载酒店列表…</p>
      </div>

      <template v-else>
        <!-- 搜索框 -->
        <div class="mt-6 flex gap-2">
          <Input
            v-model="keyword"
            placeholder="请输入酒店名称或者酒店ID"
            @keyup.enter="doSearch"
          />
          <Button variant="outline" size="icon" :disabled="searching" @click="doSearch">
            <Search class="h-3.5 w-3.5" />
          </Button>
        </div>

        <!-- 酒店列表：一页 5 个，仅展示 name 与 id -->
        <div class="mt-4 rounded-xl border bg-card overflow-hidden">
          <div v-if="searching" class="h-[240px] flex items-center justify-center">
            <Loader2 class="h-4 w-4 animate-spin text-muted-foreground" />
          </div>
          <div
            v-else-if="records.length === 0"
            class="h-[240px] flex items-center justify-center text-xs text-muted-foreground"
          >
            未找到酒店
          </div>
          <template v-else>
            <button
              v-for="r in records"
              :key="r.id"
              class="w-full flex items-center justify-between px-4 py-[13px] text-left border-b last:border-b-0 transition-colors"
              :class="selected?.id === r.id ? 'bg-primary/10' : 'hover:bg-muted/50'"
              @click="selected = r"
            >
              <span class="text-[13px] text-foreground truncate">{{ r.name }}</span>
              <span class="text-[11px] text-muted-foreground shrink-0 ml-3">ID {{ r.id }}</span>
            </button>
          </template>
        </div>

        <!-- 分页 -->
        <div class="mt-3 flex items-center justify-center gap-3">
          <Button
            variant="ghost"
            size="icon"
            class="h-6 w-6"
            :disabled="pageNum <= 1 || searching"
            @click="prevPage"
          >
            <ChevronLeft class="h-3.5 w-3.5" />
          </Button>
          <span class="text-[11px] text-muted-foreground">
            第 {{ pageNum }} / {{ pages }} 页 · 共 {{ total }} 家酒店
          </span>
          <Button
            variant="ghost"
            size="icon"
            class="h-6 w-6"
            :disabled="pageNum >= pages || searching"
            @click="nextPage"
          >
            <ChevronRight class="h-3.5 w-3.5" />
          </Button>
        </div>

        <!-- 登录按钮 -->
        <Button class="w-full mt-5" :disabled="!selected || entering" @click="enter">
          <Loader2 v-if="entering" class="h-4 w-4 animate-spin mr-2" />
          {{ entering ? "正在同步酒店房间…" : "登录" }}
        </Button>
      </template>
    </div>
  </div>
</template>

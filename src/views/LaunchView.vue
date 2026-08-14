<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { api } from "@/lib/api";
import { useDeviceStore } from "@/stores/device";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import {
  Radio,
  Zap,
  Globe,
  LockKeyhole,
  RefreshCw,
  Loader2,
} from "lucide-vue-next";
import type { RunMode } from "@/types";

const { t } = useI18n();
const router = useRouter();
const deviceStore = useDeviceStore();
const { toast } = useToast();

// 默认全局扫描：免选网卡自动覆盖所有网卡，日常使用操作最少
const selectedMode = ref<RunMode>(3);
const segmentsText = ref("");
const starting = ref(false);

const modeCards = computed(() => [
  {
    mode: 3 as RunMode,
    icon: Globe,
    title: t("launch.global"),
    desc: t("launch.globalDesc"),
  },
  {
    mode: 1 as RunMode,
    icon: Radio,
    title: t("launch.normal"),
    desc: t("launch.normalDesc"),
  },
  {
    mode: 2 as RunMode,
    icon: Zap,
    title: t("launch.super"),
    desc: t("launch.superDesc"),
  },
  {
    mode: 4 as RunMode,
    icon: LockKeyhole,
    title: t("launch.lock"),
    desc: t("launch.lockDesc"),
  },
]);

onMounted(async () => {
  await deviceStore.loadInterfaces();
  // 服务已在运行（例如从监控页停止前离开又返回）：直达设备监控
  try {
    const st = await api.getServerStatus();
    deviceStore.setStatus(st);
    if (st.running) router.replace({ name: "monitor" });
  } catch {
    /* ignore */
  }
});

async function refreshInterfaces() {
  await deviceStore.loadInterfaces();
}

async function start() {
  // 全局扫描免选网卡；其余模式需先选工作网卡
  if (selectedMode.value !== 3 && !deviceStore.selectedNic) {
    toast({ title: "请先选择工作网卡", variant: "destructive" });
    return;
  }
  if (selectedMode.value === 2 && !segmentsText.value.trim()) {
    toast({ title: "超级模式需要填写目标网段", variant: "destructive" });
    return;
  }

  starting.value = true;
  try {
    deviceStore.reset();
    const segments =
      selectedMode.value === 2
        ? segmentsText.value
            .split(/[,，]/)
            .map((s) => s.trim())
            .filter(Boolean)
        : [];
    await api.startUdpServer(
      selectedMode.value === 3 ? "" : deviceStore.selectedNic,
      selectedMode.value,
      segments
    );
    deviceStore.mode = selectedMode.value;
    // 启动成功即进入设备监控；停止服务后由监控页返回本页
    router.push({ name: "monitor" });
  } catch (e) {
    toast({
      title: t("common.failed"),
      description: String(e),
      variant: "destructive",
    });
  } finally {
    starting.value = false;
  }
}
</script>

<template>
  <div class="h-full overflow-auto p-8">
    <div class="max-w-4xl mx-auto">
    <h2 class="text-xl font-semibold">{{ t("launch.title") }}</h2>
    <p class="mt-1 text-sm text-muted-foreground">
      选择网卡与工作方式后启动服务，自动进入设备监控；DHCP 服务请在设置中开启
    </p>

    <!-- 网卡选择（全局扫描免选，自动覆盖所有网卡） -->
    <Card v-if="selectedMode !== 3" class="mt-6 p-5">
      <div class="flex items-end gap-3">
        <div class="flex-1 space-y-1.5">
          <Label>{{ t("launch.interface") }}</Label>
          <Select v-model="deviceStore.selectedNic">
            <SelectTrigger placeholder="选择用于设备通信的网卡">
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="nic in deviceStore.interfaces"
                :key="nic.name + nic.ip"
                :value="nic.name"
                :disabled="!nic.up"
              >
                {{ nic.name }}（{{
                  !nic.up ? "已断开" : nic.ip || "未配置 IPv4，开 DHCP 自动配置 134.1"
                }}）
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <Button
          variant="outline"
          size="icon"
          class="mb-0.5"
          :disabled="deviceStore.interfacesLoading"
          @click="refreshInterfaces"
        >
          <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': deviceStore.interfacesLoading }" />
        </Button>
      </div>
    </Card>

    <!-- 模式卡片 -->
    <div class="mt-6 grid grid-cols-2 gap-4">
      <Card
        v-for="c in modeCards"
        :key="c.mode"
        class="p-5 cursor-pointer transition-all hover:shadow-md"
        :class="
          selectedMode === c.mode
            ? 'ring-2 ring-primary border-primary/60 bg-primary/5'
            : 'hover:border-primary/30'
        "
        @click="selectedMode = c.mode"
      >
        <div class="flex items-center gap-3">
          <div
            class="h-10 w-10 rounded-lg flex items-center justify-center shrink-0"
            :class="selectedMode === c.mode ? 'bg-primary text-primary-foreground' : 'bg-muted'"
          >
            <component :is="c.icon" class="h-5 w-5" />
          </div>
          <div>
            <div class="font-medium text-sm">{{ c.title }}</div>
            <div class="text-xs text-muted-foreground mt-0.5">{{ c.desc }}</div>
          </div>
        </div>

        <!-- 超级模式：网段输入 -->
        <div v-if="c.mode === 2 && selectedMode === 2" class="mt-4 space-y-1.5">
          <Label class="text-xs">{{ t("launch.segments") }}</Label>
          <Input
            v-model="segmentsText"
            :placeholder="t('launch.segmentsPlaceholder')"
            @click.stop
          />
        </div>
      </Card>
    </div>

    <div class="mt-8 flex justify-end pb-4">
      <Button size="lg" class="w-56" :disabled="starting" @click="start">
        <Loader2 v-if="starting" class="h-4 w-4 animate-spin" />
        {{ t("launch.start") }}
      </Button>
    </div>
    </div>
  </div>
</template>

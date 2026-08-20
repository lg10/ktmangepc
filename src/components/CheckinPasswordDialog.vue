<script setup lang="ts">
import { ref, watch } from "vue";
import { useCheckinStore } from "@/stores/checkin";
import { checkinBase, checkinDeviceInfo, checkinLogin } from "@/lib/checkin";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Loader2 } from "lucide-vue-next";
import type { CheckinDevice } from "@/types";

/** 入住机管理密码登录：取 token 并拉取设备详情写入 store（逐台手动输入，不落盘） */
const props = defineProps<{ device: CheckinDevice | null }>();
const open = defineModel<boolean>("open", { default: false });

const checkinStore = useCheckinStore();
const { toast } = useToast();

const password = ref("");
const busy = ref(false);
const error = ref("");

watch(open, (v) => {
  if (v) {
    password.value = "";
    error.value = "";
  }
});

async function submit() {
  if (!props.device || !password.value.trim()) return;
  busy.value = true;
  error.value = "";
  const base = checkinBase(props.device);
  try {
    const token = await checkinLogin(base, password.value.trim());
    const info = await checkinDeviceInfo(base, token);
    checkinStore.setSession(props.device.fullName, token, info);
    toast({ title: `已连接 ${props.device.name}`, variant: "success" });
    open.value = false;
  } catch (e) {
    error.value = String(e instanceof Error ? e.message : e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-sm">
      <DialogTitle>登录入住机</DialogTitle>
      <DialogDescription>
        {{ device ? `${device.name} · ${device.ip}:${device.port}` : "" }}
        管理密码与设备配置页同源（缺省 123456，现网多为 000000）
      </DialogDescription>
      <form class="space-y-3" @submit.prevent="submit">
        <div class="space-y-1.5">
          <Label>管理密码</Label>
          <Input
            v-model="password"
            type="password"
            placeholder="输入设备管理密码"
            autofocus
          />
        </div>
        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
        <div class="flex justify-end gap-2 pt-1">
          <Button type="button" variant="outline" :disabled="busy" @click="open = false">
            取消
          </Button>
          <Button type="submit" :disabled="busy || !password.trim()">
            <Loader2 v-if="busy" class="h-4 w-4 animate-spin" />
            登录
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>

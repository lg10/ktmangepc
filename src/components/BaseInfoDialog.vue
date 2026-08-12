<script setup lang="ts">
import { reactive, watch } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "@/lib/api";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Loader2 } from "lucide-vue-next";
import type { RcuDevice } from "@/types";
import { ref } from "vue";

const props = defineProps<{ device: RcuDevice | null }>();
const open = defineModel<boolean>("open", { default: false });

const { t } = useI18n();
const { toast } = useToast();

// baseNum 格式：hotelId/build/floor/room/doorModel
const form = reactive({
  hotelId: "0",
  buildNum: "0",
  floorNum: "0",
  roomNum: "0",
  doorModel: "0",
});

watch(open, (v) => {
  if (v && props.device?.baseNum) {
    const parts = props.device.baseNum.split("/");
    if (parts.length === 5) {
      [form.hotelId, form.buildNum, form.floorNum, form.roomNum, form.doorModel] = parts;
    }
  }
});

const sending = ref(false);

async function send() {
  if (!props.device) return;
  sending.value = true;
  try {
    await api.sendBaseInfo(
      props.device.equipId,
      Number(form.hotelId) || 0,
      Number(form.buildNum) || 0,
      Number(form.floorNum) || 0,
      Number(form.roomNum) || 0,
      Number(form.doorModel) || 0
    );
    toast({ title: t("common.success"), description: "基础信息已下发", variant: "success" });
    open.value = false;
  } catch (e) {
    toast({ title: t("common.failed"), description: String(e), variant: "destructive" });
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogTitle>房间基础信息</DialogTitle>
      <p class="text-xs text-muted-foreground font-mono">
        {{ props.device?.equipId }} · {{ props.device?.ip }}
      </p>

      <div class="grid grid-cols-2 gap-3 mt-2">
        <div class="space-y-1.5 col-span-2">
          <Label>项目 ID</Label>
          <Input v-model="form.hotelId" type="number" />
        </div>
        <div class="space-y-1.5">
          <Label>楼栋号</Label>
          <Input v-model="form.buildNum" type="number" />
        </div>
        <div class="space-y-1.5">
          <Label>楼层号</Label>
          <Input v-model="form.floorNum" type="number" />
        </div>
        <div class="space-y-1.5">
          <Label>房间号</Label>
          <Input v-model="form.roomNum" type="number" />
        </div>
        <div class="space-y-1.5">
          <Label>户型</Label>
          <Input v-model="form.doorModel" type="number" />
        </div>
      </div>

      <div class="flex justify-end gap-2 mt-2">
        <Button variant="ghost" @click="open = false">{{ t("common.cancel") }}</Button>
        <Button :disabled="sending" @click="send">
          <Loader2 v-if="sending" class="h-4 w-4 animate-spin" />
          下发
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>

<script setup lang="ts">
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

/** 通用确认弹窗：标题 + 说明 + 取消/确认 */
defineProps<{
  open: boolean;
  title: string;
  description?: string;
  confirmText?: string;
  cancelText?: string;
  /** 确认按钮是否使用危险样式 */
  danger?: boolean;
}>();
const emit = defineEmits<{
  (e: "update:open", v: boolean): void;
  (e: "confirm"): void;
  (e: "cancel"): void;
}>();
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogContent class="max-w-sm" :show-close="false">
      <div class="space-y-2.5">
        <DialogTitle class="text-base">{{ title }}</DialogTitle>
        <DialogDescription v-if="description" class="text-sm leading-relaxed">
          {{ description }}
        </DialogDescription>
        <div class="flex justify-end gap-2 pt-1">
          <Button
            variant="outline"
            size="sm"
            @click="emit('cancel'); emit('update:open', false)"
          >
            {{ cancelText ?? "取消" }}
          </Button>
          <Button
            size="sm"
            :variant="danger ? 'destructive' : 'default'"
            @click="emit('confirm'); emit('update:open', false)"
          >
            {{ confirmText ?? "确认" }}
          </Button>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>

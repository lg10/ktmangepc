<script setup lang="ts">
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { guardOpen, guardFailed, guardError, tryStop, forceQuit } from "@/lib/exitGuard";
import { Loader2 } from "lucide-vue-next";

// 退出 / 退出登录等待弹窗：恢复期间禁止 ESC / 点遮罩关闭，只能等待或强制退出
</script>

<template>
  <Dialog :open="guardOpen">
    <DialogContent
      class="max-w-sm"
      :show-close="false"
      @escape-key-down.prevent
      @pointer-down-outside.prevent
    >
      <div class="space-y-2.5">
        <DialogTitle class="text-base">正在恢复网络配置</DialogTitle>
        <template v-if="!guardFailed">
          <DialogDescription class="text-sm leading-relaxed flex items-start gap-2">
            <Loader2 class="h-4 w-4 mt-0.5 animate-spin shrink-0" />
            <span>正在停止内置 DHCP 与网络中继并还原网卡，请稍候…</span>
          </DialogDescription>
        </template>
        <template v-else>
          <DialogDescription class="text-sm leading-relaxed break-all">
            恢复失败：{{ guardError }}
          </DialogDescription>
          <p class="text-xs text-muted-foreground leading-relaxed">
            可重试恢复；或选择直接退出（特权助手若在运行，看门狗会在 90 秒内自动还原网络）。
          </p>
          <div class="flex justify-end gap-2 pt-1">
            <Button variant="outline" size="sm" @click="forceQuit">直接退出</Button>
            <Button size="sm" @click="tryStop">重试恢复</Button>
          </div>
        </template>
      </div>
    </DialogContent>
  </Dialog>
</template>

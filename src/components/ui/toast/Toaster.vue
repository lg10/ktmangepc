<script setup lang="ts">
import { useToast } from "./use-toast";
import { cn } from "@/lib/utils";
import { X } from "lucide-vue-next";

const { toasts, dismiss } = useToast();
</script>

<template>
  <div class="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 w-80">
    <TransitionGroup
      enter-active-class="transition-all duration-200"
      enter-from-class="opacity-0 translate-x-8"
      leave-active-class="transition-all duration-200"
      leave-to-class="opacity-0 translate-x-8"
    >
      <div
        v-for="t in toasts"
        :key="t.id"
        :class="
          cn(
            'pointer-events-auto relative flex w-full items-start justify-between gap-3 overflow-hidden rounded-md border p-4 pr-8 shadow-lg',
            t.variant === 'default' && 'bg-background text-foreground',
            t.variant === 'success' && 'border-success/40 bg-background',
            t.variant === 'destructive' &&
              'border-destructive/40 bg-background text-destructive'
          )
        "
      >
        <div class="grid gap-1">
          <div class="text-sm font-semibold">{{ t.title }}</div>
          <div v-if="t.description" class="text-xs text-muted-foreground">
            {{ t.description }}
          </div>
        </div>
        <button
          class="absolute right-2 top-2 rounded-sm opacity-60 hover:opacity-100"
          @click="dismiss(t.id)"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

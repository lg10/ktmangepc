<script setup lang="ts">
import { SelectContent, SelectPortal, SelectViewport } from "reka-ui";
import { cn } from "@/lib/utils";

defineProps<{ class?: string }>();
</script>

<template>
  <SelectPortal>
    <SelectContent
      :side-offset="4"
      position="popper"
      :class="
        cn(
          'relative z-50 max-h-96 min-w-32 overflow-hidden rounded-md border bg-popover text-popover-foreground shadow-md data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
          $props.class
        )
      "
    >
      <SelectViewport class="p-1">
        <slot />
      </SelectViewport>
    </SelectContent>
  </SelectPortal>
</template>

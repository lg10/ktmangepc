<script setup lang="ts">
import { Separator } from "reka-ui";
import { cn } from "@/lib/utils";

defineProps<{ class?: string; vertical?: boolean }>();
</script>

<template>
  <Separator
    :orientation="vertical ? 'vertical' : 'horizontal'"
    :class="
      cn(
        'shrink-0 bg-border',
        vertical ? 'h-full w-px' : 'h-px w-full',
        $props.class
      )
    "
  />
</template>

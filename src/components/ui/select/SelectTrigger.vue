<script setup lang="ts">
import { SelectIcon, SelectTrigger, SelectValue } from "reka-ui";
import { ChevronDown } from "lucide-vue-next";
import { cn } from "@/lib/utils";

defineProps<{ class?: string; placeholder?: string }>();
</script>

<template>
  <SelectTrigger
    :class="
      cn(
        'flex h-9 w-full items-center justify-between whitespace-nowrap rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm ring-offset-background placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&>span]:truncate text-start',
        $props.class
      )
    "
  >
    <SelectValue :placeholder="placeholder" />
    <SelectIcon as-child>
      <ChevronDown class="h-4 w-4 opacity-50 shrink-0" />
    </SelectIcon>
  </SelectTrigger>
</template>

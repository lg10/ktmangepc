<script setup lang="ts">
import { computed, useSlots, type VNode } from "vue";
import { SelectItem, SelectItemIndicator, SelectItemText } from "reka-ui";
import { Check } from "lucide-vue-next";
import { cn } from "@/lib/utils";

defineProps<{ value: string; class?: string; disabled?: boolean }>();

/** 提取插槽文本作为 title：网卡名过长时单行省略号，悬浮可见完整内容 */
function vnodeText(node: unknown): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(vnodeText).join("");
  const v = node as VNode;
  if (typeof v.children === "string") return v.children;
  if (Array.isArray(v.children)) return vnodeText(v.children);
  return "";
}
const slots = useSlots();
const itemTitle = computed(() => vnodeText(slots.default?.()).trim());
</script>

<template>
  <SelectItem
    :value="value"
    :disabled="disabled"
    :class="
      cn(
        'relative flex w-full cursor-default select-none items-center rounded-sm py-1.5 pl-2 pr-8 text-sm outline-none focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50',
        $props.class
      )
    "
  >
    <span class="absolute right-2 flex h-3.5 w-3.5 items-center justify-center">
      <SelectItemIndicator>
        <Check class="h-4 w-4" />
      </SelectItemIndicator>
    </span>
    <SelectItemText class="min-w-0 flex-1 truncate" :title="itemTitle">
      <slot />
    </SelectItemText>
  </SelectItem>
</template>

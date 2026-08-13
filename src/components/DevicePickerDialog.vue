<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useDeviceStore } from "@/stores/device";
import { Search } from "lucide-vue-next";

const props = defineProps<{
  open: boolean;
  title: string;
  description?: string;
  /** 预选设备（例如从监控页多选带入） */
  preset?: string[];
}>();
const emit = defineEmits<{
  (e: "update:open", v: boolean): void;
  (e: "confirm", equipIds: string[]): void;
}>();

const deviceStore = useDeviceStore();
const selected = ref<Set<string>>(new Set());
const search = ref("");

watch(
  () => props.open,
  (open) => {
    if (open) {
      selected.value = new Set(props.preset ?? []);
      search.value = "";
    }
  }
);

const list = computed(() => {
  const kw = search.value.trim().toLowerCase();
  const all = deviceStore.deviceList;
  if (!kw) return all;
  return all.filter(
    (d) =>
      d.equipId.toLowerCase().includes(kw) ||
      d.ip.toLowerCase().includes(kw) ||
      d.roomNum.toLowerCase().includes(kw)
  );
});

const allChecked = computed(
  () => list.value.length > 0 && list.value.every((d) => selected.value.has(d.equipId))
);

function toggleAll() {
  if (allChecked.value) {
    for (const d of list.value) selected.value.delete(d.equipId);
  } else {
    for (const d of list.value) selected.value.add(d.equipId);
  }
  selected.value = new Set(selected.value);
}

function toggle(id: string) {
  if (selected.value.has(id)) selected.value.delete(id);
  else selected.value.add(id);
  selected.value = new Set(selected.value);
}

function confirm() {
  emit("confirm", [...selected.value]);
  emit("update:open", false);
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogContent class="max-w-xl">
      <div class="space-y-1">
        <DialogTitle>{{ title }}</DialogTitle>
        <DialogDescription v-if="description">{{ description }}</DialogDescription>
      </div>

      <div class="relative">
        <Search class="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
        <Input v-model="search" class="pl-9" placeholder="搜索设备 ID / IP / 房间号" />
      </div>

      <div class="h-72 overflow-auto rounded-md border">
        <table class="w-full text-xs border-collapse">
          <thead class="sticky top-0 bg-muted/80 backdrop-blur z-10">
            <tr class="text-left text-muted-foreground">
              <th class="py-2 px-3 w-10">
                <input type="checkbox" :checked="allChecked" @change="toggleAll" />
              </th>
              <th class="py-2 px-3 font-medium">设备 ID</th>
              <th class="py-2 px-3 font-medium">IP</th>
              <th class="py-2 px-3 font-medium">房间</th>
              <th class="py-2 px-3 font-medium">版本</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="d in list"
              :key="d.equipId"
              class="border-t border-border/60 cursor-pointer hover:bg-muted/50"
              :class="selected.has(d.equipId) && 'bg-primary/5'"
              @click="toggle(d.equipId)"
            >
              <td class="py-2 px-3" @click.stop>
                <input type="checkbox" :checked="selected.has(d.equipId)" @change="toggle(d.equipId)" />
              </td>
              <td class="py-2 px-3 font-mono">{{ d.equipId }}</td>
              <td class="py-2 px-3 font-mono">{{ d.ip }}</td>
              <td class="py-2 px-3">{{ !d.roomNum || d.roomNum === "0" ? "未匹配" : d.roomNum }}</td>
              <td class="py-2 px-3">{{ d.version }}</td>
            </tr>
          </tbody>
        </table>
        <div v-if="list.length === 0" class="py-10 text-center text-xs text-muted-foreground">
          没有可操作的设备（请先在工作模式中启动服务）
        </div>
      </div>

      <div class="flex items-center justify-end gap-2 pt-1">
        <span class="mr-auto text-xs text-muted-foreground">已选 {{ selected.size }} 台</span>
        <Button variant="outline" @click="emit('update:open', false)">取消</Button>
        <Button :disabled="selected.size === 0" @click="confirm">
          确定（{{ selected.size }}）
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>

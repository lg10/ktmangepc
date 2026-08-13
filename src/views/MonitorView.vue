<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { api } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { useDeviceStore } from "@/stores/device";
import { useUiStore } from "@/stores/ui";
import { useTaskStore } from "@/stores/task";
import { useTelnetStore } from "@/stores/telnet";
import { useHotelStore } from "@/stores/hotel";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import NetworkDialog from "@/components/NetworkDialog.vue";
import BaseInfoDialog from "@/components/BaseInfoDialog.vue";
import FilePickerDialog from "@/components/FilePickerDialog.vue";
import {
  Radar,
  Square,
  Search,
  Columns3,
  Upload,
  FileCog,
  Send,
  KeyRound,
  ChevronDown,
  X,
  CheckCheck,
  Copy,
  ArrowUp,
  ArrowDown,
  ArrowUpDown,
  ListFilter,
} from "lucide-vue-next";
import type { FileKind, RcuDevice } from "@/types";

const router = useRouter();

/** 列定义：key / 标题 / 压缩优先级（0 常驻 1 中 2 低） */
const COLUMNS = [
  { key: "ip", label: "IP", pri: 0 },
  { key: "equipId", label: "设备 ID", pri: 0 },
  { key: "roomNum", label: "房间", pri: 0 },
  { key: "status", label: "状态", pri: 0 },
  { key: "version", label: "版本", pri: 1 },
  { key: "model", label: "型号", pri: 1 },
  { key: "author", label: "作者", pri: 2 },
  { key: "rcuMac", label: "MAC", pri: 2 },
  { key: "baseNum", label: "基础号", pri: 2 },
  { key: "network", label: "网络", pri: 2 },
  { key: "server", label: "服务器", pri: 2 },
  { key: "runServer", label: "运行服务", pri: 2 },
] as const;
type ColKey = (typeof COLUMNS)[number]["key"];

const deviceStore = useDeviceStore();
const uiStore = useUiStore();
const taskStore = useTaskStore();
const telnetStore = useTelnetStore();
const hotelStore = useHotelStore();
const { toast } = useToast();

const searchText = ref("");
/** 分组模式：默认不分组 */
const groupMode = ref("none");
const selected = ref<Set<string>>(new Set());
const drawerDevice = ref<RcuDevice | null>(null);
const networkDialogOpen = ref(false);
const networkDialogKind = ref<"ip" | "server">("ip");
const baseInfoDialogOpen = ref(false);
const filePickerOpen = ref(false);
const filePickerKind = ref<FileKind>("firmware");
const pendingKind = ref<FileKind>("firmware");
const pendingIds = ref<string[]>([]);
const colMenuOpen = ref(false);
const moreMenuOpen = ref(false);

/** 容器宽度分层：wide ≥1080 / mid ≥880 / narrow <880 */
const tier = ref<"wide" | "mid" | "narrow">("wide");
const wrapRef = ref<HTMLElement | null>(null);
let ro: ResizeObserver | null = null;

onMounted(async () => {
  await deviceStore.refreshDevices();
  ro = new ResizeObserver(() => {
    const w = wrapRef.value?.clientWidth ?? 1200;
    tier.value = w >= 1080 ? "wide" : w >= 880 ? "mid" : "narrow";
  });
  if (wrapRef.value) ro.observe(wrapRef.value);
});

onBeforeUnmount(() => ro?.disconnect());

/** 房间号显示：未匹配（空/0）显示“未匹配” */
function roomLabel(r: string) {
  return !r || r === "0" ? "未匹配" : r;
}

/** 排序：默认按房间号升序；点击表头切换，再点同列反向 */
const sortKey = ref<ColKey>("roomNum");
const sortDir = ref<"asc" | "desc">("asc");

/** 自然比较：数字前缀按数值，否则中文 locale 比较 */
function naturalCompare(a: string, b: string) {
  const na = parseFloat(a);
  const nb = parseFloat(b);
  const aNum = !Number.isNaN(na) && /^\s*[\d.]/.test(a);
  const bNum = !Number.isNaN(nb) && /^\s*[\d.]/.test(b);
  if (aNum && bNum && na !== nb) return na - nb;
  return a.localeCompare(b, "zh-CN", { numeric: true });
}

function cmpDevices(a: RcuDevice, b: RcuDevice, key: ColKey) {
  if (key === "ip") {
    // IP 按四段数值比较
    const pa = a.ip.split(".").map(Number);
    const pb = b.ip.split(".").map(Number);
    for (let i = 0; i < 4; i++) {
      if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) - (pb[i] || 0);
    }
    return 0;
  }
  // 空值统一排最后
  const va = cellValue(a, key);
  const vb = cellValue(b, key);
  const ea = va === "—";
  const eb = vb === "—";
  if (ea !== eb) return ea ? 1 : -1;
  if (ea && eb) return a.equipId.localeCompare(b.equipId);
  if (key === "roomNum") {
    const na = parseInt(a.roomNum, 10);
    const nb = parseInt(b.roomNum, 10);
    const unmatched = (r: string, n: number) =>
      !r || r === "0" || Number.isNaN(n) ? Number.MAX_SAFE_INTEGER : n;
    const ra = unmatched(a.roomNum, na);
    const rb = unmatched(b.roomNum, nb);
    if (ra !== rb) return ra - rb;
  }
  return naturalCompare(va, vb) || a.equipId.localeCompare(b.equipId);
}

function onHeaderSort(key: ColKey) {
  if (sortKey.value === key) sortDir.value = sortDir.value === "asc" ? "desc" : "asc";
  else {
    sortKey.value = key;
    sortDir.value = "asc";
  }
}

/** 列筛选（Excel 式）：每列多选值；无记录 = 全选 */
const filters = ref<Map<ColKey, Set<string>>>(new Map());
const filterCol = ref<ColKey | null>(null);
const filterPos = ref({ left: 0, top: 0 });
const pendingSel = ref<Set<string>>(new Set());
/** 筛选面板内实时搜索关键词 */
const filterSearch = ref("");

function openFilter(key: ColKey, e: MouseEvent) {
  filterCol.value = key;
  filterSearch.value = "";
  const existing = filters.value.get(key);
  pendingSel.value = new Set(existing ? [...existing] : filterOptions.value.map((o) => o.value));
  filterPos.value = {
    left: Math.min(Math.max(8, e.clientX - 20), window.innerWidth - 268),
    top: 100,
  };
}

function applyFilter() {
  if (!filterCol.value) return;
  const all = new Set(filterOptions.value.map((o) => o.value));
  // 全选或清空都视为不筛选（与 Excel 清空=全选一致）
  if (pendingSel.value.size === 0 || pendingSel.value.size === all.size) {
    filters.value.delete(filterCol.value);
  } else {
    filters.value.set(filterCol.value, new Set(pendingSel.value));
  }
  filters.value = new Map(filters.value);
  filterCol.value = null;
}

function clearFilter() {
  if (!filterCol.value) return;
  filters.value.delete(filterCol.value);
  filters.value = new Map(filters.value);
  filterCol.value = null;
}

function toggleFilterOpt(v: string) {
  const s = new Set(pendingSel.value);
  if (s.has(v)) s.delete(v);
  else s.add(v);
  pendingSel.value = s;
}

/** 筛选面板候选值：全量设备按列去重 + 计数 */
const filterOptions = computed(() => {
  if (!filterCol.value) return [] as { value: string; count: number }[];
  const key = filterCol.value;
  const m = new Map<string, number>();
  for (const d of deviceStore.deviceList) {
    const v = cellValue(d, key);
    m.set(v, (m.get(v) ?? 0) + 1);
  }
  return [...m.entries()]
    .map(([value, count]) => ({ value, count }))
    .sort((a, b) => naturalCompare(a.value, b.value));
});

/** 筛选面板实时搜索后的可见候选项 */
const visibleFilterOptions = computed(() => {
  const kw = filterSearch.value.trim().toLowerCase();
  if (!kw) return filterOptions.value;
  return filterOptions.value.filter((o) => o.value.toLowerCase().includes(kw));
});

/** （全选）只作用于当前搜索可见的候选项（与 Excel 搜索后全选行为一致） */
function toggleAllVisible() {
  const s = new Set(pendingSel.value);
  const allSel = visibleFilterOptions.value.every((o) => s.has(o.value));
  for (const o of visibleFilterOptions.value) {
    if (allSel) s.delete(o.value);
    else s.add(o.value);
  }
  pendingSel.value = s;
}

const filteredDevices = computed(() => {
  const kw = searchText.value.trim().toLowerCase();
  let src = kw
    ? deviceStore.deviceList.filter(
        (d) =>
          d.equipId.toLowerCase().includes(kw) ||
          d.ip.toLowerCase().includes(kw) ||
          d.rcuMac.toLowerCase().includes(kw) ||
          d.roomNum.toLowerCase().includes(kw)
      )
    : [...deviceStore.deviceList];
  // 列筛选
  for (const [key, set] of filters.value) {
    src = src.filter((d) => set.has(cellValue(d, key)));
  }
  // 排序（默认房间号升序）
  const dir = sortDir.value === "asc" ? 1 : -1;
  const key = sortKey.value;
  return src.sort((a, b) => dir * cmpDevices(a, b, key));
});

/** 分组后的设备列表：不分组时单组渲染 */
const groups = computed(() => {
  const list = filteredDevices.value;
  if (groupMode.value === "none") {
    return [{ key: "__all", label: "", devices: list }];
  }
  const m = new Map<string, RcuDevice[]>();
  for (const d of list) {
    const key =
      groupMode.value === "roomType"
        ? d.roomTypeName || "未知房型"
        : groupMode.value === "building"
          ? d.buildName || "未知楼栋"
          : `${d.buildName || "未知楼栋"} / ${d.floorName || "未知楼层"}`;
    const arr = m.get(key);
    if (arr) arr.push(d);
    else m.set(key, [d]);
  }
  return [...m.entries()]
    .sort((a, b) => a[0].localeCompare(b[0], "zh-CN"))
    .map(([label, devices]) => ({ key: label, label, devices }));
});

const visibleColumns = computed(() =>
  COLUMNS.filter((c) => {
    if (uiStore.hiddenColumns.includes(c.key)) return false;
    // 压缩分层：narrow 隐藏低优先级列，mid 隐藏部分低优先级列
    if (tier.value === "narrow") return c.pri <= 1;
    if (tier.value === "mid") return c.pri <= 1 || c.key === "author";
    return true;
  })
);

const allChecked = computed(
  () =>
    filteredDevices.value.length > 0 &&
    filteredDevices.value.every((d) => selected.value.has(d.equipId))
);

function toggleAll() {
  if (allChecked.value) {
    for (const d of filteredDevices.value) selected.value.delete(d.equipId);
  } else {
    for (const d of filteredDevices.value) selected.value.add(d.equipId);
  }
  selected.value = new Set(selected.value);
}

function toggle(id: string) {
  if (selected.value.has(id)) selected.value.delete(id);
  else selected.value.add(id);
  selected.value = new Set(selected.value);
}

function openDrawer(d: RcuDevice) {
  drawerDevice.value = d;
}

async function copyId(text: string) {
  const ok = await copyText(text);
  toast(
    ok
      ? { title: "已复制", description: text }
      : { title: "复制失败", variant: "destructive" }
  );
}

function openNetwork(kind: "ip" | "server") {
  networkDialogKind.value = kind;
  networkDialogOpen.value = true;
}

/** 详情抽屉行（MAC 可复制） */
const drawerRows = computed(() => {
  const d = drawerDevice.value;
  if (!d) return [] as { label: string; value: string; copy?: string }[];
  return [
    { label: "房间号", value: roomLabel(d.roomNum) },
    { label: "房型", value: d.roomTypeName || "—" },
    { label: "栋 / 层", value: `${d.buildName || "—"} / ${d.floorName || "—"}` },
    { label: "MAC", value: d.rcuMac, copy: d.rcuMac },
    { label: "固件版本", value: d.version },
    { label: "授权", value: d.author || "—" },
    { label: "基础号", value: d.baseNum || "—" },
    { label: "网络", value: d.network || "—" },
    { label: "服务器", value: d.server || "—" },
    { label: "运行服务", value: d.runServer || "—" },
    { label: "最后心跳", value: fmtTime(d.lastSeen) },
  ];
});

function closeMenus() {
  colMenuOpen.value = false;
  moreMenuOpen.value = false;
  filterCol.value = null;
}

/* ---------------- 批量操作 ---------------- */

async function stopServer() {
  try {
    await api.stopUdpServer();
    toast({ title: "服务已停止" });
  } catch (e) {
    toast({ title: "操作失败", description: String(e), variant: "destructive" });
    return;
  }
  // 停止服务后返回工作模式页
  router.push({ name: "launch" });
}

function batchUpgrade(kind: FileKind, ids?: string[]) {
  pendingKind.value = kind;
  pendingIds.value = ids ?? [...selected.value];
  filePickerKind.value = kind;
  filePickerOpen.value = true;
}

async function onFilePicked(file: { uid: number }) {
  try {
    const n = await taskStore.start(pendingKind.value, pendingIds.value, file.uid);
    toast({
      title: pendingKind.value === "firmware" ? "固件升级任务已创建" : "配置下发任务已创建",
      description: `共 ${n} 台设备，进度见底部「任务」面板`,
    });
    uiStore.openDock("tasks");
  } catch (e) {
    toast({ title: "创建任务失败", description: String(e), variant: "destructive" });
  }
}

async function batchBaseInfo() {
  try {
    const n = await hotelStore.sendBaseInfoBatch();
    toast({ title: "基础信息下发完成", description: `已向 ${n} 台匹配设备下发` });
  } catch (e) {
    toast({ title: "下发失败", description: String(e), variant: "destructive" });
  }
}

async function batchAuth() {
  try {
    const [n, mode] = await hotelStore.sendAuthBatch();
    toast({ title: "批量授权完成", description: `已向 ${n} 台设备下发（${mode}）` });
  } catch (e) {
    toast({ title: "授权下发失败", description: String(e), variant: "destructive" });
  }
}

function openTelnet(devices: RcuDevice[]) {
  for (const d of devices) {
    telnetStore.open(d.equipId, d.ip).catch(() => {});
  }
  if (devices.length) {
    uiStore.openDock(`telnet:${devices[0].equipId}`);
  }
}

/** 单设备指令：0 退出查找 1 查找设备 2 硬件测试 3 老化测试 4 全开 5 全关 */
async function sendRevert(equipId: string, cmd: number, label: string) {
  try {
    await api.sendRevertCmd(equipId, cmd);
    toast({ title: `已下发：${label}` });
  } catch (e) {
    toast({ title: "操作失败", description: String(e), variant: "destructive" });
  }
}

function fmtTime(ts: number) {
  return new Date(ts).toLocaleTimeString("zh-CN", { hour12: false });
}

function cellValue(d: RcuDevice, key: ColKey): string {
  switch (key) {
    case "ip":
      return d.ip;
    case "equipId":
      return d.equipId;
    case "roomNum":
      return roomLabel(d.roomNum);
    case "version":
      return d.version;
    case "model":
      return d.equipmentModel;
    case "author":
      return d.author || "—";
    case "rcuMac":
      return d.rcuMac;
    case "baseNum":
      return d.baseNum || "—";
    case "network":
      return d.network || "—";
    case "server":
      return d.server || "—";
    case "runServer":
      return d.runServer || "—";
    default:
      return "";
  }
}

/** 压缩时并入「更多」的操作数量：wide 全部展开，mid 保留 4 个，narrow 保留 2 个 */
const primaryCount = computed(() =>
  tier.value === "wide" ? 99 : tier.value === "mid" ? 4 : 2
);
</script>

<template>
  <div class="h-full flex flex-col relative" @click="closeMenus">
    <!-- 工具栏 -->
    <div class="shrink-0 px-5 pt-4 pb-2.5 flex items-center gap-3 flex-wrap">
      <!-- 分组模式 -->
      <Select v-model="groupMode">
        <SelectTrigger class="w-36 h-9 text-xs">
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="none">不分组</SelectItem>
          <SelectItem value="roomType">按房型分组</SelectItem>
          <SelectItem value="building">按楼栋分组</SelectItem>
          <SelectItem value="buildingFloor">按楼栋楼层分组</SelectItem>
        </SelectContent>
      </Select>

      <div class="relative w-64 min-w-40 flex-1 max-w-80">
        <Search class="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
        <Input v-model="searchText" class="pl-9" placeholder="搜索 IP / 设备 ID / MAC / 房间" />
      </div>

      <div class="ml-auto flex items-center gap-2">
        <Badge variant="outline">
          {{ filteredDevices.length }}
        </Badge>

        <!-- 列筛选（屏幕管理） -->
        <div class="relative" @click.stop>
          <Button variant="outline" size="sm" @click="colMenuOpen = !colMenuOpen">
            <Columns3 class="h-3.5 w-3.5" />
            <span class="hidden min-[980px]:inline">列</span>
            <ChevronDown class="h-3 w-3" />
          </Button>
          <div
            v-if="colMenuOpen"
            class="absolute right-0 top-9 z-30 w-44 rounded-lg border bg-popover shadow-lg p-1.5"
          >
            <div
              v-for="c in COLUMNS"
              :key="c.key"
              class="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-accent cursor-pointer"
              @click="uiStore.setColumnVisible(c.key, uiStore.hiddenColumns.includes(c.key))"
            >
              <span
                class="h-3.5 w-3.5 rounded border flex items-center justify-center"
                :class="uiStore.hiddenColumns.includes(c.key) ? 'border-border' : 'bg-primary border-primary'"
              >
                <CheckCheck v-if="!uiStore.hiddenColumns.includes(c.key)" class="h-2.5 w-2.5 text-primary-foreground" />
              </span>
              {{ c.label }}
            </div>
          </div>
        </div>

        <Button variant="outline" size="sm" @click="stopServer">
          <Square class="h-3.5 w-3.5" />
          <span class="hidden min-[980px]:inline">停止服务</span>
        </Button>
      </div>
    </div>

    <!-- 批量操作栏（选中后出现） -->
    <Transition
      enter-active-class="transition-all duration-150"
      enter-from-class="opacity-0 -translate-y-1"
      leave-active-class="transition-all duration-150"
      leave-to-class="opacity-0 -translate-y-1"
    >
      <div
        v-if="selected.size > 0"
        class="shrink-0 mx-5 mb-2 rounded-lg border bg-primary/5 px-3 py-2 flex items-center gap-1.5 flex-wrap"
        @click.stop
      >
        <Badge variant="secondary" class="mr-1">已选 {{ selected.size }} 台</Badge>

        <template v-for="(btn, i) in [
          { label: '批量升级', icon: Upload, run: () => batchUpgrade('firmware') },
          { label: '批量配置', icon: FileCog, run: () => batchUpgrade('config') },
          { label: '基础信息', icon: Send, run: batchBaseInfo },
          { label: '批量授权', icon: KeyRound, run: batchAuth },
        ]" :key="btn.label">
          <Button
            v-if="i < primaryCount"
            variant="outline"
            size="sm"
            class="h-7 text-xs bg-background"
            @click="btn.run"
          >
            <component :is="btn.icon" class="h-3.5 w-3.5" />
            {{ btn.label }}
          </Button>
        </template>

        <!-- 压缩时的「更多」收纳 -->
        <div v-if="primaryCount < 5" class="relative">
          <Button variant="outline" size="sm" class="h-7 text-xs bg-background" @click="moreMenuOpen = !moreMenuOpen">
            更多
            <ChevronDown class="h-3 w-3" />
          </Button>
          <div
            v-if="moreMenuOpen"
            class="absolute left-0 top-8 z-30 w-36 rounded-lg border bg-popover shadow-lg p-1.5"
          >
            <button
              v-for="btn in [
                { label: '批量升级', icon: Upload, run: () => batchUpgrade('firmware') },
                { label: '批量配置', icon: FileCog, run: () => batchUpgrade('config') },
                { label: '基础信息', icon: Send, run: batchBaseInfo },
                { label: '批量授权', icon: KeyRound, run: batchAuth },
              ].slice(primaryCount)"
              :key="btn.label"
              class="w-full flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-accent"
              @click="btn.run(); moreMenuOpen = false"
            >
              <component :is="btn.icon" class="h-3.5 w-3.5" />
              {{ btn.label }}
            </button>
          </div>
        </div>

        <button
          class="ml-auto text-muted-foreground hover:text-foreground transition-colors p-1"
          title="取消选择"
          @click="selected = new Set()"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
    </Transition>

    <!-- 设备表格（按分组渲染） -->
    <div ref="wrapRef" class="flex-1 min-h-0 overflow-auto px-5 pb-5">
      <div v-for="g in groups" :key="g.key" class="mb-6 last:mb-0">
        <!-- 分组标题 -->
        <div v-if="groupMode !== 'none'" class="pt-3 pb-2 flex items-center gap-2">
          <span class="text-xs font-semibold">{{ g.label }}</span>
          <Badge variant="outline">{{ g.devices.length }} 台</Badge>
        </div>
        <table class="w-full text-sm border-collapse">
          <thead class="sticky top-0 bg-background z-10">
            <tr class="text-left text-xs text-muted-foreground border-b">
              <th class="py-2.5 px-3 w-10">
                <input type="checkbox" :checked="allChecked" title="全选" @change="toggleAll" />
              </th>
              <th
                v-for="c in visibleColumns"
                :key="c.key"
                class="py-2.5 px-3 font-medium whitespace-nowrap"
              >
                <span
                  class="cursor-pointer hover:text-foreground transition-colors"
                  :class="sortKey === c.key && 'text-primary'"
                  :title="`点击按${c.label}排序`"
                  @click="onHeaderSort(c.key)"
                >
                  {{ c.label }}
                </span>
                <button
                  class="ml-1 align-middle p-0.5 rounded hover:text-foreground transition-colors"
                  :class="sortKey === c.key ? 'text-primary' : 'text-muted-foreground/50 hover:text-muted-foreground'"
                  :title="`按${c.label}排序`"
                  @click.stop="onHeaderSort(c.key)"
                >
                  <ArrowUp v-if="sortKey === c.key && sortDir === 'asc'" class="h-3 w-3" />
                  <ArrowDown v-else-if="sortKey === c.key" class="h-3 w-3" />
                  <ArrowUpDown v-else class="h-3 w-3" />
                </button>
                <button
                  class="ml-1 align-middle p-0.5 rounded hover:text-foreground transition-colors"
                  :class="filters.has(c.key) ? 'text-primary' : 'text-muted-foreground/50 hover:text-muted-foreground'"
                  :title="`筛选${c.label}`"
                  @click.stop="openFilter(c.key, $event)"
                >
                  <ListFilter class="h-3 w-3" />
                </button>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="d in g.devices"
              :key="d.equipId"
              class="border-b border-border/60 cursor-pointer transition-colors"
              :class="selected.has(d.equipId) ? 'bg-primary/8' : 'hover:bg-muted/50'"
              @click="openDrawer(d)"
            >
              <td class="py-2 px-3" @click.stop>
                <input
                  type="checkbox"
                  :checked="selected.has(d.equipId)"
                  @change="toggle(d.equipId)"
                />
              </td>
              <td
                v-for="c in visibleColumns"
                :key="c.key"
                class="py-2 px-3 text-xs whitespace-nowrap"
                :class="['ip', 'equipId', 'rcuMac'].includes(c.key) && 'font-mono'"
              >
                <Badge v-if="c.key === 'status'" :variant="d.count === '正常' ? 'success' : 'warning'">
                  {{ d.count }}
                </Badge>
                <span v-else-if="c.key === 'equipId'" class="group/cp inline-flex items-center gap-1">
                  {{ d.equipId }}
                  <button
                    class="opacity-0 group-hover/cp:opacity-100 p-0.5 text-muted-foreground hover:text-foreground transition-opacity"
                    title="复制设备 ID"
                    @click.stop="copyId(d.equipId)"
                  >
                    <Copy class="h-3 w-3" />
                  </button>
                </span>
                <template v-else>{{ cellValue(d, c.key) }}</template>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div
        v-if="filteredDevices.length === 0"
        class="mt-20 flex flex-col items-center gap-2 text-muted-foreground"
      >
        <Radar class="h-8 w-8 opacity-40" />
        <p class="text-sm">暂无设备。请确认服务已启动并等待设备应答</p>
      </div>
    </div>

    <!-- Excel 式列筛选面板：多选值 + 计数，确定后筛选 -->
    <div
      v-if="filterCol"
      class="fixed z-40 w-64 rounded-lg border bg-popover shadow-lg flex flex-col"
      :style="{ left: filterPos.left + 'px', top: filterPos.top + 'px' }"
      @click.stop
    >
      <div class="px-3 py-2 border-b text-xs font-medium">
        筛选：{{ COLUMNS.find((c) => c.key === filterCol)?.label }}
      </div>
      <div class="px-3 py-1.5 border-b">
        <input
          v-model="filterSearch"
          class="w-full h-7 px-2 rounded-md bg-background border border-input text-xs outline-none focus:border-ring placeholder:text-muted-foreground/60"
          placeholder="搜索候选项…"
        />
      </div>
      <div class="px-3 py-1.5 border-b">
        <label class="flex items-center gap-2 text-xs cursor-pointer hover:text-foreground">
          <input
            type="checkbox"
            :checked="visibleFilterOptions.length > 0 && visibleFilterOptions.every((o) => pendingSel.has(o.value))"
            :indeterminate.prop="visibleFilterOptions.some((o) => pendingSel.has(o.value)) && !visibleFilterOptions.every((o) => pendingSel.has(o.value))"
            :disabled="visibleFilterOptions.length === 0"
            @change="toggleAllVisible"
          />
          （全选）
        </label>
      </div>
      <div class="max-h-64 overflow-auto py-1">
        <label
          v-for="opt in visibleFilterOptions"
          :key="opt.value"
          class="flex items-center gap-2 px-3 py-1 text-xs cursor-pointer hover:bg-accent"
        >
          <input
            type="checkbox"
            :checked="pendingSel.has(opt.value)"
            @change="toggleFilterOpt(opt.value)"
          />
          <span class="truncate max-w-36" :title="opt.value">{{ opt.value }}</span>
          <span class="ml-auto shrink-0 text-muted-foreground">({{ opt.count }})</span>
        </label>
        <div v-if="!visibleFilterOptions.length" class="px-3 py-2 text-xs text-muted-foreground">
          {{ filterOptions.length ? "无匹配候选项" : "暂无可选值" }}
        </div>
      </div>
      <div class="px-3 py-2 border-t flex items-center gap-2">
        <Button variant="ghost" size="sm" class="h-7 text-xs" @click="clearFilter">
          清除筛选
        </Button>
        <Button size="sm" class="h-7 text-xs ml-auto" @click="applyFilter">确定</Button>
      </div>
    </div>

    <!-- 抽屉外点击关闭 -->
    <div v-if="drawerDevice" class="absolute inset-0 z-10" @click="drawerDevice = null" />

    <!-- 设备详情抽屉（覆盖式，不挤压表格） -->
    <Transition
      enter-active-class="transition-transform duration-200"
      enter-from-class="translate-x-full"
      leave-active-class="transition-transform duration-200"
      leave-to-class="translate-x-full"
    >
      <aside
        v-if="drawerDevice"
        class="absolute top-0 right-0 bottom-0 w-[380px] max-w-[85%] z-20 border-l bg-card shadow-xl flex flex-col"
      >
        <div class="flex items-center gap-2 border-b px-4 py-3">
          <div class="min-w-0">
            <div class="text-sm font-semibold font-mono truncate flex items-center gap-1">
              {{ drawerDevice.equipId }}
              <button
                class="shrink-0 p-0.5 text-muted-foreground hover:text-foreground"
                title="复制设备 ID"
                @click="copyId(drawerDevice.equipId)"
              >
                <Copy class="h-3 w-3" />
              </button>
            </div>
            <div class="text-[11px] text-muted-foreground">
              {{ drawerDevice.ip }} · {{ drawerDevice.equipmentModel }}
            </div>
          </div>
          <Badge
            class="ml-auto shrink-0"
            :variant="drawerDevice.count === '正常' ? 'success' : 'warning'"
          >
            {{ drawerDevice.count }}
          </Badge>
          <button
            class="shrink-0 text-muted-foreground hover:text-foreground p-1"
            @click="drawerDevice = null"
          >
            <X class="h-4 w-4" />
          </button>
        </div>

        <div class="flex-1 overflow-auto px-4 py-3 space-y-1.5 text-xs">
          <div
            v-for="row in drawerRows"
            :key="row.label"
            class="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2"
          >
            <span class="text-muted-foreground">{{ row.label }}</span>
            <span class="font-medium text-right break-all max-w-52 select-text inline-flex items-center gap-1">
              {{ row.value }}
              <button
                v-if="row.copy"
                class="shrink-0 p-0.5 text-muted-foreground hover:text-foreground"
                :title="`复制${row.label}`"
                @click="copyId(row.copy!)"
              >
                <Copy class="h-3 w-3" />
              </button>
            </span>
          </div>
        </div>

        <div class="border-t px-4 py-3 grid grid-cols-2 gap-2">
          <Button variant="outline" size="sm" @click="baseInfoDialogOpen = true">基础信息</Button>
          <Button variant="outline" size="sm" @click="openNetwork('ip')">IP 配置</Button>
          <Button variant="outline" size="sm" @click="openNetwork('server')">服务器配置</Button>
          <Button variant="outline" size="sm" @click="batchUpgrade('firmware', [drawerDevice.equipId])">
            固件升级
          </Button>
          <Button variant="outline" size="sm" @click="batchUpgrade('config', [drawerDevice.equipId])">
            配置升级
          </Button>
          <Button variant="outline" size="sm" @click="openTelnet([drawerDevice])">Telnet</Button>
          <Button variant="ghost" size="sm" @click="sendRevert(drawerDevice.equipId, 1, '查找设备')">
            查找设备
          </Button>
          <Button variant="ghost" size="sm" @click="sendRevert(drawerDevice.equipId, 0, '退出查找')">
            退出查找
          </Button>
          <Button variant="ghost" size="sm" @click="sendRevert(drawerDevice.equipId, 2, '硬件测试')">
            硬件测试
          </Button>
          <Button variant="ghost" size="sm" @click="sendRevert(drawerDevice.equipId, 3, '老化测试')">
            老化测试
          </Button>
          <Button variant="ghost" size="sm" @click="sendRevert(drawerDevice.equipId, 4, '全开')">
            全开
          </Button>
          <Button variant="ghost" size="sm" @click="sendRevert(drawerDevice.equipId, 5, '全关')">
            全关
          </Button>
        </div>
      </aside>
    </Transition>

    <NetworkDialog v-model:open="networkDialogOpen" :kind="networkDialogKind" :device="drawerDevice" />
    <BaseInfoDialog v-model:open="baseInfoDialogOpen" :device="drawerDevice" />
    <FilePickerDialog
      v-model:open="filePickerOpen"
      :kind="filePickerKind"
      :title="filePickerKind === 'firmware' ? '选择固件文件' : '选择配置文件'"
      :description="`将向 ${pendingIds.length} 台设备${filePickerKind === 'firmware' ? '升级' : '下发'}`"
      @confirm="onFilePicked"
    />
  </div>
</template>

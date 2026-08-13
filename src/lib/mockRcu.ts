/**
 * RCU 设备模拟器（演示/联调用）
 *
 * 仅在以 `VITE_MOCK_RCU=1 pnpm tauri dev` 启动时生效：
 * - 该变量经 vite 静态替换，未启用时入口条件恒为 false，本模块不会被加载；
 * - 正常 `pnpm build` / `pnpm tauri dev` 完全不加载、不执行任何逻辑。
 *
 * 工作方式：服务启动（模式 1/2）后，按真实后端事件契约向前端
 * 发射 `udp://device` 事件，模拟 RCU 设备陆续上线与心跳刷新，
 * 使设备监控列表/分组模式有数据可看。
 */
import { emit } from "@tauri-apps/api/event";
import { EVENTS } from "@/lib/api";
import { useDeviceStore } from "@/stores/device";
import type { RcuDevice } from "@/types";

const BUILDS = ["1号楼", "2号楼", "3号楼"];
const FLOORS = ["2层", "3层", "5层", "6层"];
const ROOM_TYPES = ["大床房", "双床房", "豪华套房", "亲子房"];
const MODELS = ["大板机", "模块机", "模块一体机", "模块机【24款】", "LORA主机"];
const AUTHORS = ["永久授权", "到期授权"];

const DEVICE_TOTAL = 24;

let timers: ReturnType<typeof setInterval | typeof setTimeout>[] = [];
let active = false;
let pool: RcuDevice[] = [];
let store: ReturnType<typeof useDeviceStore> | null = null;

function pick<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

function hex(n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) s += "0123456789ABCDEF"[Math.floor(Math.random() * 16)];
  return s;
}

function makeDevice(i: number): RcuDevice {
  const build = BUILDS[i % BUILDS.length];
  const floor = FLOORS[(i + 1) % FLOORS.length];
  const roomNo = `${floor.charAt(0)}${String((i % 18) + 1).padStart(2, "0")}`;
  const ip = `192.168.134.${10 + i}`;
  const dhcp = i % 4 === 0;
  return {
    equipId: hex(16),
    roomNum: roomNo,
    roomTypeName: pick(ROOM_TYPES),
    buildName: build,
    floorName: floor,
    rcuMac: `00:1E:C0:${hex(2)}:${hex(2)}:${hex(2)}`,
    equipmentModel: pick(MODELS),
    version: `2.${1 + (i % 3)}.${300 + i}`,
    author: pick(AUTHORS),
    baseNum: `1001/${BUILDS.indexOf(build) + 1}/${FLOORS.indexOf(floor) + 1}/${roomNo}/1`,
    network: dhcp
      ? `【DHCP】${ip}\n【网/掩】192.168.134.1/255.255.255.0`
      : `【静态】${ip}\n【网/掩】192.168.134.1/255.255.255.0`,
    ip,
    server: `【IP方式】192.168.134.1:4668\n【DNS】223.5.5.5`,
    runServer: "192.168.134.1:4668",
    ipFlag: dhcp ? 0 : 1,
    mask: "255.255.255.0",
    gateway: "192.168.134.1",
    dns: "223.5.5.5",
    serverFlag: 1,
    serverUrl: "",
    serverIp: "192.168.134.1",
    serverPort: 4668,
    count: "正常",
    lastSeen: Date.now(),
  };
}

function log(line: string) {
  void emit(EVENTS.UDP_LOG, `[MOCK] ${line}`);
}

function heartbeat() {
  const now = Date.now();
  for (const d of pool) {
    // 随机少量设备呈现掉线计数，其余刷新心跳
    if (Math.random() < 0.08) {
      const n = 1 + Math.floor(Math.random() * 5);
      d.count = `[${n}/6] 掉线`;
    } else {
      d.count = "正常";
      d.lastSeen = now;
    }
  }
  // 自愈：列表被外部清空（如监控页 refreshDevices）时补发缺失设备
  if (store) {
    for (const d of pool) {
      if (!store.devices.has(d.equipId)) void emit(EVENTS.DEVICE, { ...d });
    }
  }
  // 每次心跳重发部分设备，模拟真实上报节奏
  for (const d of [...pool].sort(() => Math.random() - 0.5).slice(0, 6)) {
    void emit(EVENTS.DEVICE, { ...d });
  }
}

function begin() {
  active = true;
  pool = Array.from({ length: DEVICE_TOTAL }, (_, i) => makeDevice(i));
  log(`模拟 ${DEVICE_TOTAL} 台 RCU 设备开始上线`);
  pool.forEach((d, i) => {
    timers.push(
      setTimeout(() => {
        void emit(EVENTS.DEVICE, { ...d });
      }, 180 * i)
    );
  });
  timers.push(setInterval(heartbeat, 5000));
}

function stop() {
  active = false;
  for (const t of timers) clearTimeout(t);
  timers = [];
  pool = [];
}

/** 入口：由 App.vue 在 VITE_MOCK_RCU=1 时动态 import 调用 */
export function startMockRcu() {
  store = useDeviceStore();
  log("RCU 模拟器已加载（VITE_MOCK_RCU=1）");
  store.$subscribe((_m, state) => {
    const on = state.status.running && (state.status.mode === 1 || state.status.mode === 2);
    if (on && !active) begin();
    else if (!on && active) stop();
  });
}

import { defineStore } from "pinia";
import { api, EVENTS } from "@/lib/api";
import type {
  LockPacket,
  NetInterface,
  RcuDevice,
  RunMode,
  ServerStatus,
} from "@/types";

export const useDeviceStore = defineStore("device", {
  state: () => ({
    interfaces: [] as NetInterface[],
    selectedIp: "",
    mode: 0 as RunMode | 0,
    segments: [] as string[],
    status: {
      running: false,
      mode: 0,
      ip: "",
      port: 0,
      deviceCount: 0,
    } as ServerStatus,
    devices: new Map<string, RcuDevice>(),
    selectedEquipId: "",
    lockPackets: [] as LockPacket[],
    logs: [] as string[],
  }),
  getters: {
    deviceList: (s): RcuDevice[] =>
      [...s.devices.values()].sort((a, b) => a.ip.localeCompare(b.ip)),
    filteredLockPackets: (s) => s.lockPackets.slice(-500),
  },
  actions: {
    async loadInterfaces() {
      this.interfaces = await api.listInterfaces();
      if (!this.selectedIp && this.interfaces.length > 0) {
        this.selectedIp = this.interfaces[0].ip;
      }
    },
    upsertDevice(dev: RcuDevice) {
      // 去重过滤：同 MAC 或同 IP 视为同一设备（设备重启/换 ID 时不重复添加行）
      if (!this.devices.has(dev.equipId)) {
        for (const [id, d] of [...this.devices]) {
          const sameMac = !!dev.rcuMac && d.rcuMac === dev.rcuMac;
          const sameIp = !!dev.ip && d.ip === dev.ip;
          if (sameMac || sameIp) {
            this.devices.delete(id);
            if (this.selectedEquipId === id) this.selectedEquipId = dev.equipId;
          }
        }
      }
      this.devices.set(dev.equipId, dev);
      this.status.deviceCount = this.devices.size;
    },
    removeDevice(equipId: string) {
      this.devices.delete(equipId);
      this.status.deviceCount = this.devices.size;
      if (this.selectedEquipId === equipId) this.selectedEquipId = "";
    },
    async refreshDevices() {
      const list = await api.listDevices();
      this.devices = new Map(list.map((d) => [d.equipId, d]));
    },
    pushLockPacket(p: LockPacket) {
      this.lockPackets.push(p);
      if (this.lockPackets.length > 1000) {
        this.lockPackets.splice(0, this.lockPackets.length - 1000);
      }
    },
    pushLog(line: string) {
      this.logs.push(line);
      if (this.logs.length > 500) this.logs.splice(0, this.logs.length - 500);
    },
    setStatus(status: ServerStatus) {
      this.status = status;
    },
    reset() {
      this.devices.clear();
      this.lockPackets = [];
      this.selectedEquipId = "";
      this.mode = 0;
    },
  },
});

export { EVENTS };

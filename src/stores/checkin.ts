import { defineStore } from "pinia";
import type { CheckinDevice, CheckinDeviceInfo, CheckinStatus } from "@/types";

/** 已登录设备的会话（token 30 分钟滑动续期，仅存内存不落盘） */
interface CheckinSession {
  token: string;
  info: CheckinDeviceInfo | null;
  loginAt: number;
}

/**
 * 入住机扫描状态：mDNS 发现列表 + 登录会话
 * 后端事件 checkin://device / device-offline / status 由 App.vue 统一转发
 */
export const useCheckinStore = defineStore("checkin", {
  state: () => ({
    running: false,
    devices: [] as CheckinDevice[],
    /** fullName → 会话 */
    sessions: {} as Record<string, CheckinSession>,
  }),
  getters: {
    deviceCount: (s) => s.devices.length,
  },
  actions: {
    setStatus(st: CheckinStatus) {
      this.running = st.running;
      // 停止后设备数归零，列表以事件/列表接口为准
      if (!st.running) this.devices = [];
    },
    upsertDevice(dev: CheckinDevice) {
      const idx = this.devices.findIndex((d) => d.fullName === dev.fullName);
      if (idx >= 0) this.devices[idx] = dev;
      else this.devices.push(dev);
    },
    removeDevice(fullName: string) {
      this.devices = this.devices.filter((d) => d.fullName !== fullName);
      delete this.sessions[fullName];
    },
    setDevices(list: CheckinDevice[]) {
      this.devices = list;
    },
    setSession(fullName: string, token: string, info: CheckinDeviceInfo | null) {
      this.sessions[fullName] = { token, info, loginAt: Date.now() };
    },
    setSessionInfo(fullName: string, info: CheckinDeviceInfo) {
      const s = this.sessions[fullName];
      if (s) s.info = info;
    },
    clearSession(fullName: string) {
      delete this.sessions[fullName];
    },
    reset() {
      this.devices = [];
      this.sessions = {};
    },
  },
});

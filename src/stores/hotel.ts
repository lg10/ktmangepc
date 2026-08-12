import { defineStore } from "pinia";
import { api } from "@/lib/api";
import type { AuthInfo, HotelState } from "@/types";

/** 酒店同步 / 批量基础信息 / 批量授权 */
export const useHotelStore = defineStore("hotel", {
  state: () => ({
    hotelId: Number(localStorage.getItem("kt.hotelId") || 0),
    hotelName: localStorage.getItem("kt.hotelName") || "",
    state: { synced: false, hotelId: 0, roomCount: 0, rooms: [] } as HotelState,
    authInfo: null as AuthInfo | null,
    syncing: false,
  }),
  actions: {
    async loadState() {
      this.state = await api.getHotelState();
      if (this.state.hotelId > 0) this.hotelId = this.state.hotelId;
    },
    setHotel(id: number, name: string) {
      this.hotelId = id;
      this.hotelName = name;
      localStorage.setItem("kt.hotelId", String(id));
      localStorage.setItem("kt.hotelName", name);
    },
    async sync() {
      this.syncing = true;
      try {
        const count = await api.syncHotel(this.hotelId);
        await this.loadState();
        return count;
      } finally {
        this.syncing = false;
      }
    },
    /** 选中酒店后进入：同步房间 + 拉取授权信息 */
    async enter(id: number, name: string) {
      this.setHotel(id, name);
      const count = await this.sync();
      await this.fetchAuthInfo().catch(() => {});
      return count;
    },
    async sendBaseInfoBatch() {
      return api.sendBaseInfoBatch();
    },
    async fetchAuthInfo() {
      this.authInfo = await api.getAuthInfo(this.hotelId);
      return this.authInfo;
    },
    async sendAuthBatch() {
      return api.sendAuthBatch();
    },
  },
});

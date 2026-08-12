import { defineStore } from "pinia";
import { api } from "@/lib/api";

export interface TelnetTab {
  /** 会话键 = equipId */
  id: string;
  ip: string;
  port: number;
  /** connecting / connected / closed */
  status: string;
  reason?: string;
}

/**
 * Telnet 多会话：按 RCU 多开、可单独关闭
 * 终端缓冲由组件内 xterm 实例持有，这里只管会话生命周期
 */
export const useTelnetStore = defineStore("telnet", {
  state: () => ({
    tabs: [] as TelnetTab[],
  }),
  actions: {
    async open(equipId: string, ip: string, port = 23) {
      const exist = this.tabs.find((t) => t.id === equipId);
      if (exist) {
        exist.status = exist.status === "closed" ? "connecting" : exist.status;
        if (exist.status === "connecting") {
          await this.connect(exist);
        }
        return;
      }
      const tab: TelnetTab = { id: equipId, ip, port, status: "connecting" };
      this.tabs.push(tab);
      await this.connect(tab);
    },
    async connect(tab: TelnetTab) {
      try {
        await api.telnetConnect(tab.id, tab.ip, tab.port);
        tab.status = "connected";
      } catch (e) {
        tab.status = "closed";
        tab.reason = String(e);
      }
    },
    markClosed(id: string, reason: string) {
      const tab = this.tabs.find((t) => t.id === id);
      if (tab) {
        tab.status = "closed";
        tab.reason = reason;
      }
    },
    async close(id: string) {
      await api.telnetClose(id).catch(() => {});
      this.tabs = this.tabs.filter((t) => t.id !== id);
    },
    async reconnect(id: string) {
      const tab = this.tabs.find((t) => t.id === id);
      if (!tab) return;
      tab.status = "connecting";
      tab.reason = undefined;
      await this.connect(tab);
    },
  },
});

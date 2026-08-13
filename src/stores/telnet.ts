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
      // 必须通过 this.tabs 取回响应式代理对象再改状态：
      // 直接改 push 前的裸对象不会触发 Vue 依赖更新（会永远卡在 connecting）
      const t = this.tabs.find((x) => x.id === tab.id) ?? tab;
      try {
        await api.telnetConnect(t.id, t.ip, t.port);
        t.status = "connected";
        t.reason = undefined;
      } catch (e) {
        t.status = "closed";
        t.reason = String(e);
      }
    },
    /** 收到数据即证明会话存活（真实状态驱动，兼容 invoke 时序异常） */
    markConnected(id: string) {
      const tab = this.tabs.find((t) => t.id === id);
      if (tab && tab.status !== "connected") {
        tab.status = "connected";
        tab.reason = undefined;
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

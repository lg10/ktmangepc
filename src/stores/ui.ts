import { defineStore } from "pinia";

/**
 * UI 布局状态（侧栏折叠 / Dock 开关与高度 / 屏幕列筛选 / 终端输入模式）
 * 全部持久化到 localStorage，重启后恢复
 */
const KEY = "kt.ui.v1";

export type TerminalInputMode = "bar" | "raw";

interface PersistedUi {
  sidebarCollapsed: boolean;
  dockOpen: boolean;
  dockHeight: number;
  dockTab: string;
  hiddenColumns: string[];
  terminalInputMode: TerminalInputMode;
}

function load(): PersistedUi {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const saved = JSON.parse(raw) as Partial<PersistedUi>;
      const merged = { ...defaults(), ...saved };
      // 旧数据没有输入模式字段：说明还没加过快捷命令条，一次性把 Dock 加高，保证日志可视区不变小
      if (!("terminalInputMode" in saved)) {
        merged.dockHeight = Math.min(520, Math.max(merged.dockHeight + 32, 300));
        localStorage.setItem(KEY, JSON.stringify(merged));
      }
      return merged;
    }
  } catch {
    /* 忽略损坏的本地数据 */
  }
  return defaults();
}

function defaults(): PersistedUi {
  return {
    sidebarCollapsed: false,
    dockOpen: false,
    dockHeight: 300,
    dockTab: "tasks",
    hiddenColumns: [],
    terminalInputMode: "bar",
  };
}

export const useUiStore = defineStore("ui", {
  state: () => ({
    ...load(),
    paletteOpen: false,
    /** 文件库 / 设置改为弹窗形式，避免路由切换影响正在进行的设备扫描 */
    filesOpen: false,
    settingsOpen: false,
    /** ADB 终端弹窗（不持久化：关闭即销毁 shell 会话） */
    adbTerminalOpen: false,
  }),
  actions: {
    openFiles() {
      this.filesOpen = true;
      this.settingsOpen = false;
    },
    openSettings() {
      this.settingsOpen = true;
      this.filesOpen = false;
    },
    persist() {
      const { sidebarCollapsed, dockOpen, dockHeight, dockTab, hiddenColumns, terminalInputMode } = this;
      localStorage.setItem(
        KEY,
        JSON.stringify({ sidebarCollapsed, dockOpen, dockHeight, dockTab, hiddenColumns, terminalInputMode })
      );
    },
    toggleSidebar() {
      this.sidebarCollapsed = !this.sidebarCollapsed;
      this.persist();
    },
    setSidebar(collapsed: boolean) {
      this.sidebarCollapsed = collapsed;
      this.persist();
    },
    toggleDock() {
      this.dockOpen = !this.dockOpen;
      this.persist();
    },
    openDock(tab: string) {
      this.dockOpen = true;
      this.dockTab = tab;
      this.persist();
    },
    setDockTab(tab: string) {
      this.dockTab = tab;
      this.persist();
    },
    setDockHeight(h: number) {
      this.dockHeight = Math.min(Math.max(h, 140), 520);
      this.persist();
    },
    setTerminalInputMode(mode: TerminalInputMode) {
      this.terminalInputMode = mode;
      this.persist();
    },
    setColumnVisible(key: string, visible: boolean) {
      const set = new Set(this.hiddenColumns);
      if (visible) set.delete(key);
      else set.add(key);
      this.hiddenColumns = [...set];
      this.persist();
    },
  },
});

import { defineStore } from "pinia";

export interface QuickCommand {
  id: string;
  name: string;
  command: string;
}

export const MAX_QUICK_COMMANDS = 6;
const KEY = "kt.quickCommands.v1";

function load(): QuickCommand[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter(
        (x): x is QuickCommand =>
          x && typeof x.id === "string" && typeof x.name === "string" && typeof x.command === "string"
      )
      .slice(0, MAX_QUICK_COMMANDS);
  } catch {
    return [];
  }
}

function persist(items: QuickCommand[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(items));
  } catch {
    /* 本地存储不可用时仅当前会话生效 */
  }
}

function newId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export const useQuickCommandStore = defineStore("quickCommands", {
  state: () => ({ items: load() }),
  actions: {
    add(name: string, command: string): boolean {
      const n = name.trim();
      const c = command.trim();
      if (!n || !c || this.items.length >= MAX_QUICK_COMMANDS) return false;
      this.items.push({ id: newId(), name: n, command: c });
      persist(this.items);
      return true;
    },
    remove(id: string) {
      this.items = this.items.filter((item) => item.id !== id);
      persist(this.items);
    },
  },
});

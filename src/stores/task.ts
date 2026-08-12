import { defineStore } from "pinia";
import { api } from "@/lib/api";
import type { FileKind, UpdateTask } from "@/types";

/** 升级 / 配置任务状态（订阅 task://update 事件刷新） */
export const useTaskStore = defineStore("task", {
  state: () => ({
    tasks: [] as UpdateTask[],
  }),
  getters: {
    activeCount: (s) => s.tasks.filter((t) => t.state <= 1).length,
    firmwareTasks: (s) => s.tasks.filter((t) => t.kind === "firmware"),
    configTasks: (s) => s.tasks.filter((t) => t.kind === "config"),
  },
  actions: {
    setTasks(tasks: UpdateTask[]) {
      this.tasks = tasks;
    },
    async refresh() {
      this.tasks = await api.listUpdateTasks();
    },
    async start(kind: FileKind, equipIds: string[], uid: number) {
      return api.startFileUpdate(kind, equipIds, uid);
    },
    async cancel(equipId: string, kind: FileKind) {
      await api.cancelUpdateTask(equipId, kind);
    },
  },
});

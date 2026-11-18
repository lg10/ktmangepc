import { defineStore } from "pinia";
import { api } from "@/lib/api";
import type { FileKind, UpdateTask, UpdateTaskProgress } from "@/types";

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
    /** 增量进度：只改对应行，避免整表重渲染；已完成行不参与 patch */
    patchProgress(updates: UpdateTaskProgress[]) {
      for (const u of updates) {
        const t = this.tasks.find(
          (x) => x.kind === u.kind && x.equipId === u.equipId && x.startTime === u.startTime
        );
        if (!t || t.state > 1) continue;
        t.state = u.state;
        t.percent = u.percent;
        t.progress = u.progress;
      }
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
    /** 清除已完成记录（后端清除后由 task://update 事件同步刷新） */
    async clearFinished() {
      await api.clearUpdateTasks();
    },
  },
});

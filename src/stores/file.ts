import { defineStore } from "pinia";
import { api } from "@/lib/api";
import type { FetchProgress, FileEntry, FileKind } from "@/types";

/** 文件库：固件 / 配置文件（云端拉取入库） */
export const useFileStore = defineStore("file", {
  state: () => ({
    firmware: [] as FileEntry[],
    config: [] as FileEntry[],
    fetching: false,
    progress: { stage: "", percent: 0, message: "" } as FetchProgress,
  }),
  actions: {
    async refresh(kind: FileKind, size?: number) {
      const list = await api.listFiles(kind, size);
      if (kind === "firmware") this.firmware = list;
      else this.config = list;
    },
    async fetch(kind: FileKind, url: string) {
      this.fetching = true;
      this.progress = { stage: "info", percent: 5, message: "正在获取文件信息" };
      try {
        const entry = await api.fetchFile(kind, url);
        await this.refresh(kind);
        return entry;
      } finally {
        this.fetching = false;
      }
    },
    setProgress(p: FetchProgress) {
      this.progress = p;
    },
    async remove(kind: FileKind, uid: number) {
      await api.deleteFile(kind, uid);
      await this.refresh(kind);
    },
  },
});

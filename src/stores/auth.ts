import { defineStore } from "pinia";
import { api } from "@/lib/api";
import type { UserInfo } from "@/types";

export const useAuthStore = defineStore("auth", {
  state: () => ({
    user: null as UserInfo | null,
    checking: false,
  }),
  getters: {
    isLogin: (s) => !!s.user,
    nickName: (s) => s.user?.userInfo?.nickName ?? "",
  },
  actions: {
    async restore() {
      this.checking = true;
      try {
        this.user = await api.getLoginState();
        // 每次启动都拉云端最新用户信息校验：过期/失效立即清除登录态，避免假登录
        if (this.user) {
          try {
            this.user = await api.refreshUser();
          } catch {
            await api.logout().catch(() => {});
            this.user = null;
          }
        }
      } catch {
        this.user = null;
      } finally {
        this.checking = false;
      }
      return this.isLogin;
    },
    setUser(user: UserInfo) {
      this.user = user;
    },
    async logout() {
      try {
        await api.logout();
      } finally {
        this.user = null;
      }
    },
  },
});

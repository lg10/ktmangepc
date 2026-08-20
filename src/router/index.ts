import { createRouter, createWebHashHistory } from "vue-router";

const router = createRouter({
  // Tauri 桌面端使用 hash 路由，避免自定义协议下 history 模式的路径问题
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "splash",
      component: () => import("@/views/SplashView.vue"),
    },
    {
      path: "/login",
      name: "login",
      component: () => import("@/views/LoginView.vue"),
    },
    {
      path: "/welcome",
      name: "welcome",
      component: () => import("@/views/HotelSelectView.vue"),
    },
    {
      // 入住机管理台内嵌窗口（独立 WebviewWindow 加载，不走登录态）
      path: "/browser",
      name: "browser",
      component: () => import("@/views/BrowserView.vue"),
    },
    {
      path: "/app",
      component: () => import("@/layouts/WorkbenchLayout.vue"),
      children: [
        {
          path: "",
          name: "launch",
          component: () => import("@/views/LaunchView.vue"),
        },
        {
          path: "monitor",
          name: "monitor",
          component: () => import("@/views/MonitorView.vue"),
        },
        {
          path: "checkin",
          name: "checkin",
          component: () => import("@/views/CheckinView.vue"),
        },
        // 文件库 / 设置已改为弹窗形式（uiStore.filesOpen/settingsOpen），
        // 旧 hash 链接兼容：重定向回工作模式
        { path: "files", redirect: { name: "launch" } },
        { path: "settings", redirect: { name: "launch" } },
      ],
    },
  ],
});

export default router;

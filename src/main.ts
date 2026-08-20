import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import i18n from "./i18n";
import "./assets/index.css";
import { splashFinish, splashLog, splashProgress } from "./lib/splash";

// 运行时模块已加载：清除 index.html 的心跳日志并汇报真实阶段
const hb = (window as unknown as { __ktSplashHeartbeat?: number }).__ktSplashHeartbeat;
if (hb) window.clearInterval(hb);
splashLog("运行时模块就绪，初始化 Vue 应用…");
splashProgress(32);

const app = createApp(App);

app.use(createPinia());
app.use(router);
app.use(i18n);

app.mount("#app");

// 启动流程之外的窗口（如入住机管理台窗 /#/browser）没有业务页负责关闭闪屏，
// 初始路由就绪后直接移除，避免原生闪屏覆盖内容并拦截鼠标事件
router.isReady().then(() => {
  const name = router.currentRoute.value.name;
  if (name !== "splash" && name !== "login" && name !== "welcome") {
    splashFinish();
  }
});

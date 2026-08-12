import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import i18n from "./i18n";
import "./assets/index.css";
import { splashLog, splashProgress } from "./lib/splash";

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

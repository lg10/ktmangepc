# 肯天玉佩（mangePc）

酒店智能设备管理工具 —— 由 Avalonia（kt-tool-2.0）迁移至 **Tauri 2.0** 的跨平台版本（macOS / Windows / Linux）。

- **前端**：Vue 3 + TypeScript + Vite + Pinia + vue-router + vue-i18n
- **UI**：TailwindCSS 4 + reka-ui（headless）+ shadcn-vue 风格组件，浅色极简商务风
- **后端**：Rust（tokio UDP、rusqlite、reqwest）

## 开发运行

```bash
pnpm install
pnpm tauri dev        # 启动桌面应用（Splash 检查窗 → 登录 → 工作台）
```

前端单独调试：`pnpm dev`（端口 1420）；类型检查与构建：`pnpm build`。

## 功能入口

| 模式 | 说明 |
| --- | --- |
| 普通扫描 | 广播 0xFE00 发现报文（端口 4667），监听 4668+ 接收 RCU 应答 |
| 超级模式 | 额外对指定网段并发探测后单播发现（跨网段设备） |
| 门锁扫描 | 监听 8787 端口，记录门锁上报报文（HEX） |
| DHCP 直连 | 本机作为 DHCP 服务器（67 端口，需管理员/root），为网线直连设备分配 192.168.134.x；自动模式会在检测到真实网络时跳过启动 |

设备监控页支持：网络配置下发（0xFE10）、房间基础信息下发（0xFE12）、UdpRevert 指令（查找/硬件测试/老化测试/全开/全关）。

## 无硬件联调：设备模拟器

没有真实 RCU 时，使用协议级模拟器完成全链路验证：

```bash
cd tools/rcu-simulator
cargo run -- --dev-ip 192.168.134.88 --dev-port 4668
```

联调步骤：

1. 启动模拟器（默认指令端口 4668；若主程序占用可换端口并同步理解为主程序顺延绑定）。
2. `pnpm tauri dev` 启动主程序，登录后选择网卡（任意 IPv4 网卡即可，广播走本机协议栈），进入**普通扫描**。
3. 监控表格应在 3 秒内出现设备 `AABBCCDD11223344`（MINI主机，192.168.134.88）。
4. 选中设备 → 下发"基础信息"或"网络配置"，模拟器控制台会打印更新结果并回发最新设备信息，表格随之刷新。
5. 协议回归自测（不依赖主程序）：`python3 tools/rcu-simulator/selftest.py`（需模拟器以 `--dev-port 4699` 运行）。

## DHCP 权限说明

- macOS/Linux：绑定 67 端口需 root；普通权限下启动会给出明确提示。
- Windows：需以管理员身份运行。
- 自动模式逻辑：存在非 192.168.134.x 的活跃 IPv4 接口或存在默认路由 → 视为有真实网络，拒绝自动开启（避免与现网 DHCP 冲突）。

## 打包

```bash
pnpm tauri build      # macOS: dmg/app；Windows: msi+nsis；Linux: deb/AppImage
```

## 自建更新服务（Tauri Updater v2 适配）

```bash
cd tools/update-server
node server.js        # 默认 9123 端口，PORT 环境变量可改
```

- 客户端请求：`GET /update/{target}/{arch}/{currentVersion}`，无更新返回 204，有更新返回 Tauri Updater 标准 JSON（version/notes/pub_date/url/signature）。
- 签名用 `pnpm tauri signer generate` 生成密钥对，构建时签名安装包，公钥配置到 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`，endpoint 指向本服务。
- 发布新版本时更新 `releases.json` 并上传安装包与 `.sig` 到 CDN。

## 阶段二规划

固件升级（0x02F2-F4）、配置下发（0xFD01-03）、批量授权（0xF102）、Telnet 终端（xterm）、屏幕管理、酒店同步、文件库下载分包 MD5、守护任务、Tauri Updater 自建更新服务端。

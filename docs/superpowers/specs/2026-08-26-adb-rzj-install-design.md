# ADB 终端弹窗：已连接设备列表与入住机一键安装 设计文档

日期：2026-08-26
状态：已确认（方案 A）

## 背景与目标

在现有 ADB 终端弹窗（`AdbTerminalDialog.vue`）标题行右上角新增「已连接设备」按钮，点击后在弹窗内右侧滑出设备面板，展示内置 adb 已连接的设备列表（序列号 / 状态 / 连接方式）。正常连接（`device`）状态的设备提供「安装入住机」按钮：点击后从 `https://d.kingint.com/app/rzj/lastest.json` 拉取可安装版本列表供选择，确认后进入「下载 → 安装 → 拉起」流水线，按钮位置以进度条展示下载与安装进度。

## 已确认的需求决策

| 决策点 | 结论 |
|---|---|
| 清单接口 | `https://d.kingint.com/app/rzj/lastest.json`（注意是 lastest 拼写）；`install` 为对象映射，条目含 `name` / `version` / `url` |
| 拉起方式 | 包名固定 `com.kingint.checkin4`，安装后 `adb shell monkey -p com.kingint.checkin4 -c android.intent.category.LAUNCHER 1` |
| 设备名称 | 仅展示序列号（不查 getprop） |
| 连接方式 | 序列号含 `:` 判为网络（tcp），否则 USB |
| 刷新机制 | 打开面板自动查一次，之后手动刷新 |
| 面板形式 | 弹窗内右侧滑出面板（不新建窗口、不嵌套 Dialog） |
| 下载策略 | 同 URL 共享下载缓存；缓存放 AppData 下 `rzj-cache/`，**下载新 URL 前清空目录内其他 APK**，避免版本升级后旧缓存堆积无法删除 |
| 安装按钮可见性 | 仅 `status == device` 的行显示 |

## 后端设计（新增 `src-tauri/src/rzj.rs`）

### 命令

1. **`rzj_devices() -> Vec<RzjDevice>`**
   `std::process::Command` 直接执行内置 `<adb> devices`（复用 `adbshell.rs` 的 `resolve_adb_dir` 定位逻辑，将其提取为 `pub(crate)`）。解析输出，跳过 `adb server started...` 等干扰行，仅取 `序列号<tab>状态` 行。

   ```rust
   struct RzjDevice { serial: String, status: String, transport: String } // transport: "usb" | "tcp"
   ```

2. **`rzj_releases() -> Vec<RzjRelease>`**
   reqwest GET 清单 JSON，解析 `install` 映射为数组（`key` / `name` / `version` / `url`）。

3. **`rzj_install(serial, url) -> Result<(), String>`**
   tokio spawn 异步任务：
   - **缓存**：目标文件已存在则跳过下载；否则清空 `rzj-cache/` 中其他 APK → 下载到 `<name>.part` → 完成后原子改名
   - **安装**：`adb -s <serial> install -r <apk>`，流式读 stdout，解析进度百分比行
   - **拉起**：上述包名固定 `com.kingint.checkin4`，安装成功后执行 `monkey` 拉起；拉起失败不视为任务失败（`done` + message 附警告）
   - **互斥**：同设备进行中时拒绝重复发起
   - 服务状态挂 `AppState`（`state.rs` 加 `pub rzj: Arc<RzjService>`），命令在 `lib.rs` 注册

### 共享下载去重

服务内 `Mutex<HashMap<String, Arc<OnceCell<Result<PathBuf, String>>>>>`（tokio OnceCell），按 URL 去重：先到者下载，其余等待同一结果。下载进度按 serial 维度推送：服务为每个 URL 维护等待者（serial）列表，实际下载者每收到一段进度就向列表内所有 serial 各发一次 `download` 阶段事件，保证每个设备行的进度条都能动。缓存文件名取 URL 末段（如 `rzj-online-4.2.2.apk`）。

### 进度事件 `rzj://progress`

`events.rs` 与 `api.ts` EVENTS 同步新增 `RZJ_PROGRESS`：

```rust
struct RzjProgress {
    serial: String,
    stage: String,   // download | install | launch | done | error
    percent: u8,     // 0-100
    message: String, // 阶段说明或错误信息
}
```

### 缓存清理

- 下载新 URL 前：删除 `rzj-cache/` 内其他 `.apk`
- 服务首次使用时：清理残留 `.part`

## 前端设计

### `AdbTerminalDialog.vue`

标题行（`flex items-center px-6 pt-4 pb-2`）右侧（`ml-auto`）加「已连接设备」按钮（lucide `Smartphone` 图标），切换面板开关状态（组件内 `ref`，不持久化）。

### 新组件 `src/components/AdbDevicePanel.vue`

- `DialogContent` 内 `absolute right-0 inset-y-0 w-[360px]`，`translate-x` 过渡滑入滑出，左侧加边框，覆盖在终端区域上
- 顶栏：标题「已连接设备」+ 刷新按钮（`RefreshCw`）+ 关闭按钮
- 两种内部视图：
  - **设备列表**：每行序列号（等宽字体）+ 状态徽标（`device` 绿「已连接」/ `unauthorized` 黄「未授权」/ 其他灰「离线」）+ 连接方式徽标（USB / 网络）；`device` 状态行右侧「安装入住机」按钮；空列表显示空状态提示
  - **版本选择**：点击安装后切换，拉 `rzj_releases` 列表（名称 + 版本，单选），「确定」调 `rzj_install` 后切回设备列表；「返回」回设备列表；请求失败显示错误提示
- 进度：组件维护 `Map<serial, {stage, percent, message}>`，挂载时订阅 `RZJ_PROGRESS` 事件；任务中设备行的按钮位置替换为进度条 + 阶段文字（下载中 45% / 安装中 23%（解析不到百分比显示"安装中…"）/ 拉起应用…）；`done` 短暂显示绿色"已完成"后恢复按钮并 success toast；`error` 恢复按钮并 destructive toast（附 message）
- 关闭面板 / 关闭终端弹窗不中断后端任务；重新打开面板后，进行中任务的后续进度事件仍能更新展示

### API 层

`api.ts` 新增：`rzjDevices()` / `rzjReleases()` / `rzjInstall(serial, url)`；EVENTS 新增 `RZJ_PROGRESS: "rzj://progress"`。

## 错误处理矩阵

| 场景 | 处理 |
|---|---|
| 内置 adb 定位失败 / `adb devices` 执行失败 | 面板内错误文案，可刷新重试 |
| 设备列表为空 | 空状态"未检测到已连接设备" |
| 清单请求/解析失败 | 选择视图错误提示 + 返回按钮 |
| 下载失败 | error 事件 → 恢复按钮 + destructive toast；清理 `.part` |
| 安装失败（掉线 / `INSTALL_FAILED_*`） | error 事件 → toast 展示 stderr 关键信息 |
| 安装成功、拉起失败 | 视为 done，message 附"拉起失败"警告 |
| 同设备重复发起 | 后端拒绝："该设备正在安装" |
| 安装中设备拔线 | adb 进程报错退出 → 走错误流程 |
| 首次 `adb devices` 拉起 server 慢 | 前端加载态兜底 |

## 改动文件清单

- 新增：`src-tauri/src/rzj.rs`、`src/components/AdbDevicePanel.vue`
- 修改：`src-tauri/src/events.rs`、`src-tauri/src/state.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/adbshell.rs`（`resolve_adb_dir` 改可见性）、`src/lib/api.ts`、`src/components/AdbTerminalDialog.vue`

## 非目标（YAGNI）

- 不做自动轮询刷新
- 不做无线调试配对（wireless pairing）
- 不做多包名/多 APK 拉起的 JSON 扩展（包名当前固定）
- 不新增 Cargo 依赖、不改 capabilities（自定义命令无需权限声明）

## 测试计划

- 手动：连一台真机（USB 与网络各一）验证列表、安装全流程、进度展示、错误分支（断网下载失败、重复点击、拔线）
- 编译校验：`pnpm build` + `cargo check`

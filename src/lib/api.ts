import { invoke } from "@tauri-apps/api/core";
import type {
  AuthInfo,
  DhcpStatus,
  EnvReport,
  FileEntry,
  FileKind,
  HotelState,
  HotelSearchResult,
  LockPacket,
  NetInterface,
  NetworkConfig,
  RcuDevice,
  RunMode,
  ServerStatus,
  TelnetSession,
  UpdateInfo,
  UpdateTask,
  UserInfo,
} from "@/types";

/** 事件名常量（与 Rust 侧保持一致） */
export const EVENTS = {
  DEVICE: "udp://device",
  DEVICE_OFFLINE: "udp://device-offline",
  LOCK_PACKET: "udp://lock-packet",
  SERVER_STATUS: "udp://server-status",
  UDP_LOG: "udp://log",
  LOGIN_SUCCESS: "auth://login-success",
  LOGIN_FAILED: "auth://login-failed",
  QR_STATE: "auth://qr-state",
  SPLASH_LOG: "splash://update",
  DHCP_STATUS: "dhcp://status",
  DHCP_LEASE: "dhcp://lease",
  FILE_FETCH_PROGRESS: "file://fetch-progress",
  TASK_UPDATE: "task://update",
  TELNET_DATA: "telnet://data",
  TELNET_CLOSED: "telnet://closed",
} as const;

export const api = {
  /** 启动环境检查（Splash 页调用） */
  checkEnvironment: () => invoke<EnvReport>("check_environment"),

  /** 检查更新：拉取云端清单与当前版本比较 */
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  /** 系统默认浏览器打开外链 */
  openUrl: (url: string) => invoke<void>("open_url", { url }),

  /** 网卡列表 */
  listInterfaces: () => invoke<NetInterface[]>("list_network_interfaces"),

  /** 登录态 */
  getLoginState: () => invoke<UserInfo | null>("get_login_state"),
  /** 启动时拉取云端最新用户信息，校验登录态是否过期 */
  refreshUser: () => invoke<UserInfo>("refresh_user"),
  logout: () => invoke<void>("logout"),
  /** 开始轮询扫码登录结果 */
  beginQrLogin: (key: string) => invoke<void>("begin_qr_login", { key }),
  cancelQrLogin: () => invoke<void>("cancel_qr_login"),
  /** 获取扫码登录二维码内容 */
  buildLoginCode: (code: string) => invoke<string>("build_login_code", { code }),

  /** UDP 服务 */
  startUdpServer: (ip: string, mode: RunMode, segments: string[]) =>
    invoke<number>("start_udp_server", { ip, mode, segments }),
  stopUdpServer: () => invoke<void>("stop_udp_server"),
  getServerStatus: () => invoke<ServerStatus>("get_server_status"),
  listDevices: () => invoke<RcuDevice[]>("list_devices"),
  clearDevices: () => invoke<void>("clear_devices"),

  /** 指令下发 */
  sendNetworkConfig: (equipId: string, config: NetworkConfig) =>
    invoke<void>("send_network_config", { equipId, config }),
  /** 下发房间基础信息（项目/栋/层/房/户型） */
  sendBaseInfo: (
    equipId: string,
    hotelId: number,
    buildNum: number,
    floorNum: number,
    roomNum: number,
    doorModel: number
  ) =>
    invoke<void>("send_base_info", {
      equipId,
      hotelId,
      buildNum,
      floorNum,
      roomNum,
      doorModel,
    }),
  sendRevertCmd: (equipId: string, cmd: number) =>
    invoke<void>("send_revert_cmd", { equipId, cmd }),

  /** 门锁 */
  listLockPackets: () => invoke<LockPacket[]>("list_lock_packets"),

  /** DHCP（手动启动检测到真实网络时返回 REAL_NETWORK: 前缀错误，确认后 force 重试） */
  startDhcp: (interfaceName: string, autoMode: boolean, force = false) =>
    invoke<void>("start_dhcp", { interfaceName, autoMode, force }),
  stopDhcp: () => invoke<void>("stop_dhcp"),
  dhcpStatus: () => invoke<DhcpStatus>("get_dhcp_status"),

  /** 提权：检测当前进程权限 / 一键提权重启（macOS/Windows/Linux） */
  isElevated: () => invoke<boolean>("is_elevated"),
  restartElevated: () => invoke<void>("restart_elevated"),

  /** 文件库（云端拉取） */
  fetchFile: (kind: FileKind, url: string) =>
    invoke<FileEntry>("fetch_file", { kind, url }),
  listFiles: (kind: FileKind, size?: number) =>
    invoke<FileEntry[]>("list_files", { kind, size: size ?? null }),
  deleteFile: (kind: FileKind, uid: number) =>
    invoke<void>("delete_file", { kind, uid }),

  /** 升级 / 配置任务 */
  startFileUpdate: (kind: FileKind, equipIds: string[], uid: number) =>
    invoke<number>("start_file_update", { kind, equipIds, uid }),
  listUpdateTasks: () => invoke<UpdateTask[]>("list_update_tasks"),
  cancelUpdateTask: (equipId: string, kind: FileKind) =>
    invoke<void>("cancel_update_task", { equipId, kind }),

  /** 酒店同步 / 批量基础信息 */
  syncHotel: (hotelId: number) => invoke<number>("sync_hotel", { hotelId }),
  searchHotel: (name: string, pageNum: number, pageSize: number) =>
    invoke<HotelSearchResult>("search_hotel", { name, pageNum, pageSize }),
  getHotelState: () => invoke<HotelState>("get_hotel_state"),
  sendBaseInfoBatch: () => invoke<number>("send_base_info_batch"),

  /** 批量授权 */
  getAuthInfo: (hotelId: number) => invoke<AuthInfo>("get_auth_info", { hotelId }),
  sendAuthBatch: () => invoke<[number, string]>("send_auth_batch"),

  /** Telnet 多会话 */
  telnetConnect: (id: string, ip: string, port?: number) =>
    invoke<void>("telnet_connect", { id, ip, port: port ?? null }),
  telnetWrite: (id: string, data: number[]) =>
    invoke<void>("telnet_write", { id, data }),
  telnetClose: (id: string) => invoke<void>("telnet_close", { id }),
  telnetList: () => invoke<TelnetSession[]>("telnet_list"),
};

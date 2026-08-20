import { invoke } from "@tauri-apps/api/core";
import type {
  AuthInfo,
  CheckinDevice,
  CheckinStatus,
  DhcpLease,
  DhcpStatus,
  EnvReport,
  FileEntry,
  FileKind,
  HotelState,
  HotelSearchResult,
  InetShareStatus,
  LockPacket,
  NetInterface,
  NetworkConfig,
  RcuDevice,
  ResidueReport,
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
  INET_STATUS: "inet://status",
  EXIT_BLOCKED: "app://exit-blocked",
  FILE_FETCH_PROGRESS: "file://fetch-progress",
  TASK_UPDATE: "task://update",
  TELNET_DATA: "telnet://data",
  TELNET_CLOSED: "telnet://closed",
  CHECKIN_DEVICE: "checkin://device",
  CHECKIN_DEVICE_OFFLINE: "checkin://device-offline",
  CHECKIN_STATUS: "checkin://status",
  ADB_DATA: "adb://data",
  ADB_CLOSED: "adb://closed",
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
  startUdpServer: (interfaceName: string, mode: RunMode, segments: string[]) =>
    invoke<number>("start_udp_server", { interfaceName, mode, segments }),
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

  /** DHCP（手动启动检测到真实网络时返回 REAL_NETWORK: 前缀错误，确认后 force 重试；
   * 未提权时后端弹系统密码框拉起特权助手中继，无需重启应用） */
  startDhcp: (interfaceName: string, autoMode: boolean, force = false) =>
    invoke<void>("start_dhcp", { interfaceName, autoMode, force }),
  stopDhcp: () => invoke<void>("stop_dhcp"),
  dhcpStatus: () => invoke<DhcpStatus>("get_dhcp_status"),
  dhcpLeases: () => invoke<DhcpLease[]>("get_dhcp_leases"),

  /** 网络中继：调用系统互联网共享（Windows ICS / macOS 互联网共享），
   * 把源网卡的网络共享给目标网口给设备供网；与内置 DHCP 互斥，需系统管理员授权 */
  startInetShare: (src: string, dst: string) =>
    invoke<void>("start_inet_share", { src, dst }),
  stopInetShare: () => invoke<void>("stop_inet_share"),
  inetShareStatus: () => invoke<InetShareStatus>("get_inet_share_status"),

  /** 网卡残留检测与恢复：被 kill / 强制关机后残留的共享开关与 134.1 地址，
   * 恢复由提权助手 --nic-restore 执行（需系统管理员授权） */
  checkNicResidue: () => invoke<ResidueReport>("check_nic_residue"),
  restoreNetwork: () => invoke<string>("restore_network"),
  /** 跳过恢复直接退出进程（特权助手看门狗会兜底还原） */
  quitNow: () => invoke<void>("quit_now"),

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
  /** 清除已完成的升级 / 配置任务记录（进行中任务不受影响） */
  clearUpdateTasks: () => invoke<void>("clear_update_tasks"),

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

  /** 入住机 mDNS 发现（服务类型 _kingint-kcd._tcp） */
  checkinStart: () => invoke<void>("checkin_start"),
  checkinStop: () => invoke<void>("checkin_stop"),
  checkinStatus: () => invoke<CheckinStatus>("checkin_status"),
  checkinList: () => invoke<CheckinDevice[]>("checkin_list"),

  /** ADB 终端（内置 adb 的系统 shell 会话，关窗即销毁） */
  adbShellOpen: () => invoke<void>("adb_shell_open"),
  adbShellWrite: (data: number[]) => invoke<void>("adb_shell_write", { data }),
  adbShellClose: () => invoke<void>("adb_shell_close"),
};

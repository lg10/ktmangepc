/** 网络接口（网卡）信息 */
export interface NetInterface {
  name: string;
  ip: string;
  mac: string;
  isLoopback: boolean;
  /** 是否持有 IPv4（false 时 ip 为空，开 DHCP 会自动配置 134.1） */
  hasIpv4: boolean;
  /** 链路是否已连接（false = 已断开，灰显禁选） */
  up: boolean;
}

/** 运行模式：1 普通扫描 2 超级模式 3 全局扫描 4 门锁扫描 5 入住机扫描 */
export type RunMode = 1 | 2 | 3 | 4 | 5;

/** 入住机 mDNS 发现设备（服务类型 _kingint-kcd._tcp，端口 7271） */
export interface CheckinDevice {
  /** mDNS 实例全名（多台同名设备自动加后缀，唯一键） */
  fullName: string;
  /** 服务实例名（如 checkin / checkin (2)） */
  name: string;
  ip: string;
  port: number;
}

/** 入住机扫描服务状态 */
export interface CheckinStatus {
  running: boolean;
  deviceCount: number;
}

/** 入住机详情（登录设备管理接口后 /api/device/info 返回，字段随设备实现） */
export interface CheckinDeviceInfo {
  deviceId?: string;
  model?: string;
  version?: string;
  [key: string]: unknown;
}

/** UDP 服务状态 */
export interface ServerStatus {
  running: boolean;
  mode: RunMode | 0;
  ip: string;
  port: number;
  deviceCount: number;
}

/** RCU 设备（对应原 RcuTableModel） */
export interface RcuDevice {
  equipId: string;
  roomNum: string;
  roomTypeName: string;
  buildName: string;
  floorName: string;
  rcuMac: string;
  equipmentModel: string;
  version: string;
  author: string;
  baseNum: string;
  network: string;
  ip: string;
  server: string;
  runServer: string;
  /** 结构化参数（配置对话框回填用，勿解析显示文本） */
  ipFlag: number;
  mask: string;
  gateway: string;
  dns: string;
  serverFlag: number;
  serverUrl: string;
  serverIp: string;
  serverPort: number;
  /** 正常 / [n/6] 掉线计数 */
  count: string;
  lastSeen: number;
}

/** 门锁扫描原始报文 */
export interface LockPacket {
  srcIp: string;
  srcPort: number;
  hex: string;
  ts: number;
}

/** 用户信息（云端返回） */
export interface UserInfo {
  token: string;
  userInfo: {
    id?: number;
    username?: string;
    nickName: string;
    icon?: string;
  };
  [key: string]: unknown;
}

/** 网络配置下发参数 */
export interface NetworkConfig {
  ipFlag: number;
  ip: string;
  mask: string;
  gateway: string;
  dns: string;
  serverFlag: number;
  serverUrl: string;
  serverIp: string;
  serverPort: number;
}

/** 启动检查报告 */
export interface EnvReport {
  platform: string;
  appVersion: string;
  elevated: boolean;
  dbReady: boolean;
  internetReachable: boolean;
  loginValid: boolean;
}

/** 检查更新结果 */
export interface UpdateInfo {
  current: string;
  latest: string;
  hasUpdate: boolean;
  notes: string;
  url: string;
}

/** DHCP 状态 */
export interface DhcpStatus {
  running: boolean;
  interfaceName: string;
  serverIp: string;
  leaseCount: number;
  autoMode: boolean;
}

/** DHCP 租约 */
export interface DhcpLease {
  mac: string;
  ip: string;
  ts: number;
}

/** 网络中继（互联网共享）状态：把源网卡（如 Wi-Fi）的网络共享给目标网口给设备供网 */
export interface InetShareStatus {
  running: boolean;
  /** 源网卡（有网络的，如 Wi-Fi） */
  src: string;
  /** 目标网口（接设备的，含 USB 转接网口） */
  dst: string;
}

/** 网卡残留检测报告（被 kill / 强制关机后的异常退出残留） */
export interface ResidueReport {
  /** 上次运行未干净退出（状态标记文件残留） */
  markerFound: boolean;
  /** 系统互联网共享仍处于开启（Windows ICS / macOS 互联网共享） */
  sharingEnabled: boolean;
  /** 仍持有 192.168.134.1 副地址的网卡名 */
  residualNics: string[];
}

/** 文件类型：firmware 固件 / config 配置 */
export type FileKind = "firmware" | "config";

/** 文件库条目（云端拉取入库后） */
export interface FileEntry {
  uid: number;
  name: string;
  hotelName: string;
  version: string;
  author: string;
  custom: number;
  size: number;
  roomTypeName: string;
  hotelId: number;
  createTime: number;
  fileTime: number;
}

/** 拉取进度事件载荷 */
export interface FetchProgress {
  /** info / download / split / done / error */
  stage: string;
  percent: number;
  message: string;
}

/** 升级/配置任务（state：0 等待 1 进行 10 完成 999 超时/取消 1999 重复） */
export interface UpdateTask {
  kind: FileKind;
  equipId: string;
  uid: number;
  fileName: string;
  totalNum: number;
  percent: number;
  progress: string;
  state: number;
  startTime: number;
  endTime: number;
}

/** 酒店房间映射（同步自云端） */
export interface HotelRoom {
  equipId: string;
  roomNum: number;
  roomName: string;
  buildUid: number;
  floorUid: number;
  hotelId: number;
}

/** 酒店同步状态 */
export interface HotelState {
  synced: boolean;
  hotelId: number;
  roomCount: number;
  rooms: HotelRoom[];
}

/** 授权信息 */
export interface AuthInfo {
  permanent: boolean;
  /** 截止时间戳（秒），永久为远未来值 */
  deadline: number;
  text: string;
}

/** 酒店搜索结果项 */
export interface HotelSearchRecord {
  id: number;
  name: string;
}

/** 酒店搜索分页结果 */
export interface HotelSearchResult {
  total: number;
  current: number;
  size: number;
  records: HotelSearchRecord[];
}

/** Telnet 会话 */
export interface TelnetSession {
  id: string;
  ip: string;
  port: number;
  connected: boolean;
}

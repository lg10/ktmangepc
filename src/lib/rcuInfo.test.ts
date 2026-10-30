import test from "node:test";
import assert from "node:assert/strict";
import { displayWidth, formatRcuInfo, isRcuInfoComplete } from "./rcuInfo.ts";
import { LOG_COLORS } from "./logHighlight.ts";

// 真机 get-rcu-info 完整输出（用户提供的原样样本）
const SAMPLE = `RCU: MC9600
SN: 0001007F1901C131
runAPP: App1  1758279111(2025-09-19 18:51:51)
runAPPType: Unknown
firmware: 1758278072(2025-09-19 18:34:32)(Sep 19 2025 18:34:32)
升级时间: 2025-09-19 18:52:18
协议: 1001
软件: 240
硬件: 50
项目编号: 0
楼栋-楼层-房间: 0-0-0
lockType: 4  LicSta: 0x00
已运行0 m(0 h)   剩余1439 h  授权2160 h(90 d)
取电类型: 常供电
断电时间: 255 s   剩余：0 s
房间状态: 0
chick info invalid
force pwr invalid
取电状态: 1
房门: 1
受控插座: 1
KZ03拓展接口: 未初始化
UTC: 1793329429(2026-10-30 11:03:49)(10781 s  effective)
配置文件: 12559(2026-03-26 09:09:49)
DM：0x7F
rst: [NRST][SW]`;

const ANSI = /\x1b\[[0-9;]*m/g;
const strip = (s: string) => s.replace(ANSI, "");

test("表格：所有字段都保留，且分组标题存在", () => {
  const out = formatRcuInfo(SAMPLE);
  assert.ok(out);
  const plain = strip(out);
  for (const text of [
    "MC9600",
    "0001007F1901C131",
    "App1  1758279111(2025-09-19 18:51:51)",
    "Unknown",
    "1758278072(2025-09-19 18:34:32)(Sep 19 2025 18:34:32)",
    "2025-09-19 18:52:18",
    "1001",
    "240",
    "50",
    "0-0-0",
    "12559(2026-03-26 09:09:49)",
    "0x7F",
    "[NRST][SW]",
  ]) {
    assert.ok(plain.includes(text), `缺少字段值：${text}`);
  }
  for (const group of ["设备信息", "运行信息", "电源与门锁", "状态信息"]) {
    assert.ok(plain.includes(group), `缺少分组：${group}`);
  }
  assert.ok(plain.indexOf("App1  1758279111") < plain.lastIndexOf("└"), "runAPP 应在表格内");
});

test("表格：组合行拆成多行（lockType/LicSta、已运行/剩余/授权、断电时间/剩余）", () => {
  const plain = strip(formatRcuInfo(SAMPLE)!);
  assert.ok(plain.includes("lockType"));
  assert.ok(plain.includes("LicSta"));
  assert.ok(plain.includes("0 m(0 h)"));
  assert.ok(plain.includes("1439 h"));
  assert.ok(plain.includes("2160 h(90 d)"));
  assert.ok(plain.includes("255 s"));
  assert.ok(plain.includes("断电剩余"));
  assert.ok(plain.includes("剩余运行"));
});

test("表格：标志行进状态信息，invalid 用错误色", () => {
  const out = formatRcuInfo(SAMPLE)!;
  const plain = strip(out);
  assert.ok(plain.includes("chick info"));
  assert.ok(plain.includes("force pwr"));
  assert.ok(out.includes(`${LOG_COLORS.error}invalid${LOG_COLORS.reset}`));
});

test("表格：中文按两列宽，所有边框行等宽", () => {
  const plain = strip(formatRcuInfo(SAMPLE)!);
  const rows = plain.split(/\r?\n/).filter((l) => /[│┌├└]/.test(l));
  const widths = new Set(rows.map((l) => displayWidth(l)));
  assert.equal(widths.size, 1, `边框宽度不一致：${[...widths].join(",")}`);
  assert.equal(displayWidth("软件"), 4);
  assert.equal(displayWidth("楼栋-楼层-房间"), 14);
  assert.equal(displayWidth("："), 2);
});

test("表格：交织日志保留在表后，前缀/后缀日志保留", () => {
  const raw =
    "发送心跳--\r\n" +
    SAMPLE.replace("SN: 0001007F1901C131\n", "SN: 0001007F1901C131\nudpRxLen=84  192.168.2.13:4668\n") +
    "\r\nudp send to 192.168.2.13:4668  len=162  result=162\r\n~ # ";
  const plain = strip(formatRcuInfo(raw)!);
  const bottom = plain.lastIndexOf("└");
  assert.ok(plain.indexOf("发送心跳--") >= 0);
  assert.ok(plain.lastIndexOf("udpRxLen=84") > bottom, "命令输出中的日志应移到表后");
  assert.ok(plain.lastIndexOf("udp send to") > bottom);
  assert.ok(plain.indexOf("发送心跳--") < plain.indexOf("┌"), "前缀日志应在表前");
});

test("表格：命令回显与提示符不进入表格", () => {
  const raw = `~ # get-rcu-info\r\n${SAMPLE}\r\n~ # `;
  const plain = strip(formatRcuInfo(raw)!);
  assert.ok(!plain.includes("~ #"));
  assert.ok(!/│\s*get-rcu-info\s*│/.test(plain));
});

test("结束判定：RCU 之前的提示符不误判，rst 或其后提示符才算结束", () => {
  assert.equal(isRcuInfoComplete("~ # get-rcu-info\r\n"), false);
  assert.equal(isRcuInfoComplete("~ # get-rcu-info\r\nRCU: MC9600\r\nSN: 1\r\n"), false);
  assert.equal(isRcuInfoComplete(SAMPLE), true);
  assert.equal(isRcuInfoComplete(`${SAMPLE}\r\n~ # `), true);
  assert.equal(isRcuInfoComplete("RCU: MC9600\r\n~ # "), true);
});

test("非本命令输出或字段过少时返回 null（交给原始输出兜底）", () => {
  assert.equal(formatRcuInfo("hello\nworld"), null);
  assert.equal(formatRcuInfo("RCU: MC9600\nSN: 1"), null);
});

test("控制字符：行首 NUL 不把已知字段挤出表格", () => {
  const raw = SAMPLE.replace("runAPP:", "\u0000runAPP:");
  const plain = strip(formatRcuInfo(raw)!);
  assert.ok(plain.includes("App1  1758279111(2025-09-19 18:51:51)"));
  assert.ok(!plain.includes("其他输出"), "已知字段不应落入其他输出");
});

test("字段顺序：rst 之后出现的已知字段仍进入表格", () => {
  const raw =
    SAMPLE.replace("runAPP: App1  1758279111(2025-09-19 18:51:51)\n", "") +
    "\nrunAPP: App1  1758279111(2025-09-19 18:51:51)";
  const plain = strip(formatRcuInfo(raw)!);
  assert.ok(plain.indexOf("App1  1758279111") < plain.lastIndexOf("└"), "runAPP 应在表格内");
  assert.ok(!plain.includes("其他输出"));
});

test("超时兜底：没有 rst 但字段足够时仍可成表", () => {
  const partial = SAMPLE.replace(/\r?\nrst: \[NRST\]\[SW\]/, "");
  assert.equal(isRcuInfoComplete(partial), false);
  const plain = strip(formatRcuInfo(partial)!);
  assert.ok(plain.includes("MC9600"));
  assert.ok(plain.includes("[NRST][SW]") === false);
});

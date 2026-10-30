import test from "node:test";
import assert from "node:assert/strict";
import { parseCompletion } from "./telnetCompletion.ts";

// 真实设备（busybox）Tab 输出样本：输入 de + Tab，日志同时在刷
const SAMPLE_MULTI = [
  "de",
  "        debug-ethernet",
  "        debug-rs485-info",
  "        debug-noCardPower",
  "        debug-console-info",
  "        debug-events-execute",
  "        debug-local-rcu-relay-out",
  "        debug-time-events-execute",
  "~ # debug-device [31] offline",
  "device [32] offline",
  "device [33] offline",
  "",
].join("\n");

test("多候选：解析候选与公共前缀", () => {
  const r = parseCompletion(SAMPLE_MULTI, "de");
  assert.deepEqual(r.candidates, [
    "debug-ethernet",
    "debug-rs485-info",
    "debug-noCardPower",
    "debug-console-info",
    "debug-events-execute",
    "debug-local-rcu-relay-out",
    "debug-time-events-execute",
  ]);
  assert.equal(r.lcp, "debug-");
});

test("多候选：提示符重绘被剥离，粘连的日志保留", () => {
  const r = parseCompletion(SAMPLE_MULTI, "de");
  assert.equal(r.leftover, "device [31] offline\ndevice [32] offline\ndevice [33] offline\n");
});

test("多候选：设备行缓冲是公共前缀，清理退格数匹配", () => {
  const r = parseCompletion(SAMPLE_MULTI, "de");
  assert.equal(r.remoteLine, "debug-");
});

test("单候选：从提示符重绘里恢复补全词", () => {
  const r = parseCompletion("de\r\n~ # debug-eth ", "de");
  assert.deepEqual(r.candidates, ["debug-eth"]);
  assert.equal(r.lcp, "debug-eth");
  assert.equal(r.remoteLine, "debug-eth ");
  assert.equal(r.leftover, "");
});

test("参数补全：公共前缀拼回整行，退格清理覆盖整行", () => {
  const raw = "debug-ethernet \n        eth0\n        eth1\n~ # debug-ethernet eth\n";
  const r = parseCompletion(raw, "debug-ethernet ");
  assert.deepEqual(r.candidates, ["eth0", "eth1"]);
  assert.equal(r.lcp, "debug-ethernet eth");
  assert.equal(r.remoteLine, "debug-ethernet eth");
  assert.equal(r.leftover, "");
});

test("空输入 Tab：列全部命令，公共前缀为空，粘连日志保留", () => {
  const raw = "\n        alpha\n        beta\n~ # device [31] offline\n";
  const r = parseCompletion(raw, "");
  assert.deepEqual(r.candidates, ["alpha", "beta"]);
  assert.equal(r.lcp, "");
  assert.equal(r.remoteLine, "");
  assert.equal(r.leftover, "device [31] offline\n");
});

test("无候选：日志原样保留", () => {
  const raw = "udpRxLen=42  192.168.2.13:4668\nudp send to 192.168.2.13:4668  len=162  result=162\n";
  const r = parseCompletion(raw, "de");
  assert.deepEqual(r.candidates, []);
  assert.equal(r.lcp, "");
  assert.equal(r.leftover, raw);
});

test("无候选：Tab 可能被行缓冲吞掉，清理按发送长度 +1", () => {
  const r = parseCompletion("", "de");
  assert.equal(r.remoteLine, "de\t");
});

// 真机样本：候选以 TAB 缩进、NUL 结束；提示符重绘用 NUL 分隔字段
const REAL_MULTI =
  "de\r\n" +
  "\tdebug-ethernet\0\r\n" +
  "\tdebug-rs485-info\0\r\n" +
  "\tdebug-noCardPower\0\r\n" +
  "\tdebug-console-info\0\r\n" +
  "\tdebug-events-execute\0\r\n" +
  "\tdebug-local-rcu-relay-out\0\r\n" +
  "\tdebug-time-events-execute\0\r\n" +
  "~ # \0de\0";

test("真机多候选：TAB 缩进 + NUL 结束符", () => {
  const r = parseCompletion(REAL_MULTI, "de");
  assert.deepEqual(r.candidates, [
    "debug-ethernet",
    "debug-rs485-info",
    "debug-noCardPower",
    "debug-console-info",
    "debug-events-execute",
    "debug-local-rcu-relay-out",
    "debug-time-events-execute",
  ]);
  assert.equal(r.lcp, "debug-");
  assert.equal(r.remoteLine, "debug-");
  assert.equal(r.leftover, "");
});

test("真机多候选：NUL 字段后粘连的日志保留", () => {
  const r = parseCompletion(`${REAL_MULTI}bug-\r\n`, "de");
  assert.equal(r.leftover, "bug-");
});

test("真机多候选：候选之间插入的日志保留", () => {
  const raw =
    "de\r\n\tdebug-ethernet\0\r\n" +
    "udpRxLen=42  192.168.2.13:4668\r\n" +
    "\tdebug-console-info\0\r\n~ # \0de\0";
  const r = parseCompletion(raw, "de");
  assert.deepEqual(r.candidates, ["debug-ethernet", "debug-console-info"]);
  assert.equal(r.leftover, "udpRxLen=42  192.168.2.13:4668\n");
});

test("真机唯一补全：无提示符重绘，只回显补全后的整词", () => {
  const r = parseCompletion("debug-console-info \r\n", "debug-co");
  assert.deepEqual(r.candidates, ["debug-console-info"]);
  assert.equal(r.lcp, "debug-console-info");
  assert.equal(r.remoteLine, "debug-console-info ");
  assert.equal(r.leftover, "");
});

test("真机唯一补全：前后日志保留", () => {
  const raw =
    "udpRxLen=84  192.168.2.13:4668\r\n" +
    "debug-console-info \r\n" +
    "发送心跳--\r\n";
  const r = parseCompletion(raw, "debug-co");
  assert.deepEqual(r.candidates, ["debug-console-info"]);
  assert.equal(r.leftover, "udpRxLen=84  192.168.2.13:4668\n发送心跳--\n");
});

test("真机空输入 Tab：NUL 提示符不产出残留", () => {
  const raw = "\r\n\talpha\0\r\n\tbeta\0\r\n~ # \0\0";
  const r = parseCompletion(raw, "");
  assert.deepEqual(r.candidates, ["alpha", "beta"]);
  assert.equal(r.lcp, "");
  assert.equal(r.remoteLine, "");
  assert.equal(r.leftover, "");
});

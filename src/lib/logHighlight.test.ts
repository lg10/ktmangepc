import test from "node:test";
import assert from "node:assert/strict";
import { LogHighlighter, LOG_COLORS, highlightLine } from "./logHighlight.ts";

// 真机心跳报文（节选，白底终端上实际出现的单行 JSON）
const HEARTBEAT =
  '{"h":{"ns":"Kt.Rcu.Control","n":"HeartbeatReport","m":"0001007F1901C131-1793328631-202044",' +
  '"v":"1001-1-240","md5":"3A3B","p":{"d":[{"m":"MC9600","sn":"0001007F1901C131","protVer":"1001",' +
  '"softVer":"240","hardVer":"50","at":4,"as":0,"project":0,"build":0,"floor":0,"room":0,"power":1,' +
  '"checkIn":0,"tRun":0,"tRemain":0,"tA":2160,"cfgID":12559,"tCfg":1774487389}]},"a":[]}}';

const ANSI = /\x1b\[[0-9;]*m/g;
const strip = (s: string) => s.replace(ANSI, "");

test("JSON：键/字符串/数字/标点着色，且去掉 ANSI 后内容不变", () => {
  const out = highlightLine(HEARTBEAT);
  assert.equal(strip(out), HEARTBEAT);
  assert.ok(out.includes(`${LOG_COLORS.key}"ns"${LOG_COLORS.reset}`), "键使用键色");
  assert.ok(out.includes(`${LOG_COLORS.string}"Kt.Rcu.Control"${LOG_COLORS.reset}`), "值使用字符串色");
  assert.ok(out.includes(`${LOG_COLORS.number}2160${LOG_COLORS.reset}`), "数字使用数字色");
  assert.ok(out.includes(`${LOG_COLORS.punct}{${LOG_COLORS.reset}`), "标点使用标点色");
});

test("JSON：转义引号不截断字符串，true/false/null 单独着色", () => {
  const line = '{"a":"x\\"y","b":true,"c":null}';
  const out = highlightLine(line);
  assert.equal(strip(out), line);
  assert.ok(out.includes(`${LOG_COLORS.string}"x\\"y"${LOG_COLORS.reset}`));
  assert.ok(out.includes(`${LOG_COLORS.boolean}true${LOG_COLORS.reset}`));
  assert.ok(out.includes(`${LOG_COLORS.null}null${LOG_COLORS.reset}`));
});

test("JSON：中文前缀（原始数据：）保持原样，只给 JSON 上色", () => {
  const line = '原始数据：  {"m":"0001007F1901C131","result":1}';
  const out = highlightLine(line);
  assert.equal(strip(out), line);
  assert.ok(out.startsWith("原始数据：  "), "前缀不能被 ANSI 包裹");
  assert.ok(out.includes(`${LOG_COLORS.key}"m"${LOG_COLORS.reset}`));
});

test("普通日志行保持原样", () => {
  assert.equal(highlightLine("发送心跳--"), "发送心跳--");
  assert.equal(highlightLine("HeartbeatReport*************"), "HeartbeatReport*************");
  assert.equal(highlightLine("~ # "), "~ # ");
});

test("指标：key=value 染色且数值可读", () => {
  const line = "udpRxLen=84  192.168.2.13:4668";
  const out = highlightLine(line);
  assert.equal(strip(out), line);
  assert.ok(out.includes(`${LOG_COLORS.metricKey}udpRxLen${LOG_COLORS.reset}`));
  assert.ok(out.includes(`${LOG_COLORS.metricValue}84${LOG_COLORS.reset}`));
  assert.ok(out.includes(`${LOG_COLORS.ip}192.168.2.13:4668${LOG_COLORS.reset}`));
});

test("指标：带空格的 len = 442 与 result = 1", () => {
  const line = "send success len = 442";
  const out = highlightLine(line);
  assert.equal(strip(out), line);
  assert.ok(out.includes(`${LOG_COLORS.success}success${LOG_COLORS.reset}`));
  assert.ok(out.includes(`${LOG_COLORS.metricKey}len${LOG_COLORS.reset}`));
  assert.ok(out.includes(`${LOG_COLORS.metricValue}442${LOG_COLORS.reset}`));
});

test("关键词：offline 红色、online 绿色，方括号编号不动", () => {
  const line = "device [31] offline";
  const out = highlightLine(line);
  assert.equal(strip(out), line);
  assert.ok(out.includes(`${LOG_COLORS.error}offline${LOG_COLORS.reset}`));
  assert.ok(out.includes("[31]"));

  const online = highlightLine("device [32] online");
  assert.ok(online.includes(`${LOG_COLORS.success}online${LOG_COLORS.reset}`));
});

test("已有 ANSI 的行原样透传", () => {
  const banner = "\x1b[90m── Telnet 0001007F1901C131 (192.168.2.45) ──\x1b[0m";
  assert.equal(highlightLine(banner), banner);
});

test("高亮器：按行缓冲，跨块 JSON 拼好后再上色", () => {
  const h = new LogHighlighter();
  assert.equal(h.push('{"a":'), "");
  assert.equal(h.pending, true);
  const out = h.push("1}\nnext line\npartial");
  assert.equal(strip(out), '{"a":1}\nnext line\n');
  assert.ok(out.includes(`${LOG_COLORS.number}1${LOG_COLORS.reset}`));
  assert.equal(h.pending, true);
  assert.equal(strip(h.flush()), "partial");
  assert.equal(h.pending, false);
});

test("高亮器：CRLF 与空行内容不变", () => {
  const h = new LogHighlighter();
  const out = h.push("a\r\n\r\nb\n");
  assert.equal(strip(out), "a\r\n\r\nb\n");
});

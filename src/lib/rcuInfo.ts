import { LOG_COLORS } from "./logHighlight.ts";

const C = LOG_COLORS;
const BOLD = "\x1b[1m";

export interface CommandFormatter {
  isComplete(raw: string): boolean;
  format(raw: string, command: string): string | null;
}

/** get-rcu-info 的字段分组；行顺序在这里定义，表格按分组输出 */
const GROUPS: { title: string; keys: string[] }[] = [
  {
    title: "设备信息",
    keys: ["RCU", "SN", "协议", "软件", "硬件", "项目编号", "楼栋-楼层-房间", "配置文件", "DM"],
  },
  {
    title: "运行信息",
    keys: ["runAPP", "runAPPType", "firmware", "升级时间", "已运行", "剩余运行", "授权", "UTC"],
  },
  {
    title: "电源与门锁",
    keys: ["lockType", "LicSta", "取电类型", "断电时间", "断电剩余", "取电状态", "房门", "受控插座", "KZ03拓展接口"],
  },
  {
    title: "状态信息",
    keys: ["房间状态", "chick info", "force pwr", "rst"],
  },
];

const KNOWN_KEYS = new Set(GROUPS.flatMap((g) => g.keys));
const WIDE_CHAR = /[\u1100-\u115F\u2E80-\uA4CF\uAC00-\uD7A3\uF900-\uFAFF\uFE30-\uFE4F\uFF00-\uFF60\uFFE0-\uFFE6]/;

interface Pair {
  key: string;
  value: string;
}

export function displayWidth(text: string): number {
  let width = 0;
  for (const ch of text) width += WIDE_CHAR.test(ch) ? 2 : 1;
  return width;
}

function padRight(text: string, width: number): string {
  return text + " ".repeat(Math.max(0, width - displayWidth(text)));
}

function splitPair(segment: string): Pair | null {
  const m = /^([^:：]+?)\s*[:：]\s*(.*)$/.exec(segment.trim());
  if (!m) return null;
  return { key: m[1].trim(), value: m[2].trim() };
}

function parseLine(line: string): Pair[] | null {
  const trimmed = line.trim();
  if (!trimmed) return null;

  // 已运行0 m(0 h)   剩余1439 h  授权2160 h(90 d)
  const runtime = /^已运行(.*?)\s{2,}剩余(.*?)\s{2,}授权(.*)$/.exec(trimmed);
  if (runtime) {
    return [
      { key: "已运行", value: runtime[1].trim() },
      { key: "剩余运行", value: runtime[2].trim() },
      { key: "授权", value: runtime[3].trim() },
    ];
  }

  // 断电时间: 255 s   剩余：0 s —— 第二个“剩余”是断电剩余，避免和运行剩余重名
  if (/^断电时间\s*[:：]/.test(trimmed)) {
    const parts = trimmed.split(/\s{2,}/);
    if (parts.length >= 2 && /^剩余\s*[:：]/.test(parts[1])) {
      const first = splitPair(parts[0]);
      const second = splitPair(parts[1].replace(/^剩余/, "断电剩余"));
      if (first && second) return [first, second];
    }
  }

  // chick info invalid / force pwr invalid
  const note = /^(chick info|force pwr)\s+(.+)$/.exec(trimmed);
  if (note) return [{ key: note[1], value: note[2].trim() }];

  // lockType: 4  LicSta: 0x00（两段及以上空格，且每段都是已知 key: value；
  // 值里带时间的 runAPP 行也会被双空格切开，不能只看“像 key: value”）
  const parts = trimmed.split(/\s{2,}/);
  if (parts.length > 1) {
    const pairs = parts.map(splitPair);
    if (pairs.every((p): p is Pair => !!p && KNOWN_KEYS.has(p.key))) return pairs;
  }

  // runAPP: App1  1758279111(...) —— 值里带双空格，整行按第一个冒号切
  const single = splitPair(trimmed);
  return single ? [single] : null;
}

export function isRcuInfoComplete(raw: string): boolean {
  const start = raw.search(/\bRCU\s*[:：]/);
  if (start < 0) return false;
  const after = raw.slice(start);
  if (/(?:^|\r?\n)\s*rst\s*[:：]\s*\[NRST\]\[SW\]/.test(after)) return true;
  // 某些固件省略 rst，回到提示符即视为结束
  return /(?:^|\r?\n)\s*~ # /.test(after);
}

function paint(text: string, color: string): string {
  return color + text + C.reset;
}

function renderTable(rows: { group: string; key: string; value: string }[], title: string): string {
  const keyW = Math.max(...rows.map((r) => displayWidth(r.key)));
  const valueW = Math.max(...rows.map((r) => displayWidth(r.value)));
  const inner = keyW + valueW + 5;
  const total = inner + 2;
  const border = (text: string) => paint(text, C.punct);

  const lines: string[] = [];
  const titleWidth = displayWidth(title);
  lines.push(
    border("┌") +
      " " +
      paint(title, BOLD + C.key) +
      " " +
      border("─".repeat(Math.max(0, total - titleWidth - 4)) + "┐")
  );

  const seenGroups = new Set<string>();
  for (const row of rows) {
    if (!seenGroups.has(row.group)) {
      seenGroups.add(row.group);
      if (seenGroups.size > 1) {
        lines.push(border("├" + "─".repeat(keyW + 2) + "┴" + "─".repeat(valueW + 2) + "┤"));
      }
      lines.push(border("│") + paint(" " + padRight(row.group, inner - 1), BOLD + C.key) + border("│"));
      lines.push(border("├" + "─".repeat(keyW + 2) + "┬" + "─".repeat(valueW + 2) + "┤"));
    }
    const valuePad = " ".repeat(Math.max(0, valueW - displayWidth(row.value)));
    let valueText = row.value + valuePad;
    if (/invalid/i.test(row.value)) valueText = paint(row.value, C.error) + valuePad;
    else if (row.value === "未初始化") valueText = paint(row.value, C.number) + valuePad;
    lines.push(
      border("│") +
        " " +
        paint(padRight(row.key, keyW), C.metricKey) +
        " " +
        border("│") +
        " " +
        valueText +
        " " +
        border("│")
    );
  }
  lines.push(border("└" + "─".repeat(inner) + "┘"));
  return lines.join("\r\n");
}

/**
 * 只格式化 get-rcu-info：找不到 RCU 起始行或字段过少时返回 null，由调用方原样输出。
 * 命令块内交织的日志保留在表格后面，不丢内容。
 */
export function formatRcuInfo(raw: string, command = "get-rcu-info"): string | null {
  const before: string[] = [];
  const others: string[] = [];
  const pairs: Pair[] = [];
  let started = false;

  for (const rawLine of raw.split(/\r?\n/)) {
    // 设备偶尔在行首带 NUL 等控制符，直接 trim 不掉，会让已知字段被误判成杂项
    const line = rawLine.replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/g, "").trim();
    if (!line) continue;
    if (line.startsWith("~ #")) continue;
    if (line === command) continue;

    if (!started) {
      if (/^RCU\s*[:：]/.test(line)) started = true;
      else {
        before.push(rawLine);
        continue;
      }
    }

    const parsed = parseLine(line);
    if (!parsed || parsed.length === 0 || parsed.some((p) => !KNOWN_KEYS.has(p.key))) {
      others.push(rawLine);
      continue;
    }
    pairs.push(...parsed);
  }

  if (!pairs.some((p) => p.key === "RCU") || pairs.length < 5) return null;

  const values = new Map(pairs.map((p) => [p.key, p.value]));
  const rows: { group: string; key: string; value: string }[] = [];
  for (const group of GROUPS) {
    for (const key of group.keys) {
      const value = values.get(key);
      if (value !== undefined) rows.push({ group: group.title, key, value });
    }
  }

  const chunks: string[] = [];
  if (before.length) chunks.push(before.join("\r\n"));
  chunks.push(renderTable(rows, command));
  if (others.length) {
    chunks.push(paint("── 其他输出 ──", C.punct));
    chunks.push(others.join("\r\n"));
  }
  return chunks.join("\r\n");
}

export const rcuInfoFormatter: CommandFormatter = {
  isComplete: isRcuInfoComplete,
  format: formatRcuInfo,
};

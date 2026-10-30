/**
 * 终端日志高亮：xterm 没有内联语法高亮能力，这里在白底主题上注入 ANSI 24 位色。
 * 只负责上色，不改变日志内容（去掉 ANSI 后应与输入完全一致）。
 */
export const LOG_COLORS = {
  reset: "\x1b[0m",
  key: "\x1b[38;2;37;99;235m", // #2563eb
  string: "\x1b[38;2;21;128;61m", // #15803d
  number: "\x1b[38;2;180;83;9m", // #b45309
  boolean: "\x1b[38;2;124;58;237m", // #7c3aed
  null: "\x1b[38;2;100;116;139m", // #64748b
  punct: "\x1b[38;2;148;163;184m", // #94a3b8
  metricKey: "\x1b[38;2;100;116;139m", // #64748b
  metricValue: "\x1b[38;2;29;78;216m", // #1d4ed8
  ip: "\x1b[38;2;14;116;144m", // #0e7490
  error: "\x1b[38;2;220;38;38m", // #dc2626
  success: "\x1b[38;2;21;128;61m", // #15803d
} as const;

const C = LOG_COLORS;
const ANSI_SGR = /\x1b\[[0-9;]*m/;

function colorizePlain(text: string): string {
  if (!text) return "";
  const rule =
    /([A-Za-z_][A-Za-z0-9_]*)(\s*=\s*)(-?\d+(?:\.\d+)?)|((?:\d{1,3}\.){3}\d{1,3}(?::\d+)?)|\b(offline|error|fail(?:ed|ure)?|timeout)\b|\b(success|online)\b/gi;
  return text.replace(rule, (match, key, eq, value, ip, bad, good) => {
    if (key !== undefined) {
      return `${C.metricKey}${key}${C.reset}${eq}${C.metricValue}${value}${C.reset}`;
    }
    if (ip !== undefined) return `${C.ip}${ip}${C.reset}`;
    if (bad !== undefined) return `${C.error}${match}${C.reset}`;
    if (good !== undefined) return `${C.success}${match}${C.reset}`;
    return match;
  });
}

function findJsonStart(line: string): number {
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch !== "{" && ch !== "[") continue;
    let j = i + 1;
    while (j < line.length && /\s/.test(line[j])) j++;
    const next = line[j];
    if (next === '"' || next === "{" || next === "[" || next === "}" || next === "]") return i;
  }
  return -1;
}

/** 括号配平找 JSON 结尾；跨块的半截 JSON 只上色到行尾 */
function findJsonEnd(line: string, start: number): number {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < line.length; i++) {
    const ch = line[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "{" || ch === "[") depth++;
    else if (ch === "}" || ch === "]") {
      depth--;
      if (depth === 0) return i + 1;
    }
  }
  return line.length;
}

function highlightJson(json: string): string {
  let out = "";
  let i = 0;
  while (i < json.length) {
    const ch = json[i];
    if (ch === '"') {
      let j = i + 1;
      while (j < json.length) {
        if (json[j] === "\\") {
          j += 2;
          continue;
        }
        if (json[j] === '"') {
          j++;
          break;
        }
        j++;
      }
      const token = json.slice(i, j);
      let k = j;
      while (k < json.length && /\s/.test(json[k])) k++;
      out += (json[k] === ":" ? C.key : C.string) + token + C.reset;
      i = j;
      continue;
    }
    if (ch === "-" || (ch >= "0" && ch <= "9")) {
      const m = /^-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/.exec(json.slice(i));
      if (m) {
        out += C.number + m[0] + C.reset;
        i += m[0].length;
        continue;
      }
    }
    if (json.startsWith("true", i) || json.startsWith("false", i)) {
      const word = json.startsWith("true", i) ? "true" : "false";
      out += C.boolean + word + C.reset;
      i += word.length;
      continue;
    }
    if (json.startsWith("null", i)) {
      out += C.null + "null" + C.reset;
      i += 4;
      continue;
    }
    if ("{}[],:".includes(ch)) {
      out += C.punct + ch + C.reset;
      i++;
      continue;
    }
    out += ch;
    i++;
  }
  return out;
}

/** 单行日志高亮；已有 ANSI 的行原样返回，避免二次转义 */
export function highlightLine(line: string): string {
  if (!line || ANSI_SGR.test(line)) return line;
  const start = findJsonStart(line);
  if (start < 0) return colorizePlain(line);
  const end = findJsonEnd(line, start);
  return colorizePlain(line.slice(0, start)) + highlightJson(line.slice(start, end)) + colorizePlain(line.slice(end));
}

/** 跨 TCP 分块的日志按行缓冲：完整行立即上色，半截行最多等下一次数据或 flush */
export class LogHighlighter {
  private buf = "";

  get pending(): boolean {
    return this.buf.length > 0;
  }

  push(text: string): string {
    if (!text) return "";
    this.buf += text;
    const lastNewline = this.buf.lastIndexOf("\n");
    if (lastNewline < 0) return "";
    const chunk = this.buf.slice(0, lastNewline + 1);
    this.buf = this.buf.slice(lastNewline + 1);

    let out = "";
    let pos = 0;
    while (true) {
      const nl = chunk.indexOf("\n", pos);
      if (nl < 0) break;
      out += highlightLine(chunk.slice(pos, nl)) + "\n";
      pos = nl + 1;
    }
    return out;
  }

  flush(): string {
    if (!this.buf) return "";
    const rest = this.buf;
    this.buf = "";
    return highlightLine(rest);
  }

  reset() {
    this.buf = "";
  }
}

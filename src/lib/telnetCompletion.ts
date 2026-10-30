/** 设备 busybox 提示符（样本：`~ # `），用于剥离 Tab 后的重绘行 */
export const DEFAULT_PROMPT = "~ # ";

export interface CompletionResult {
  /** 设备列出的候选词；唯一匹配时从回显/重绘行恢复，单元素 */
  candidates: string[];
  /** 候选公共前缀（设备 Tab 后行缓冲里的内容） */
  lcp: string;
  /**
   * Tab 处理后设备行缓冲的内容。UI 按它长度发退格，避免下次回车
   * 把查询用的半截命令拼进正式命令。
   */
  remoteLine: string;
  /** 查询窗口内除候选/回显以外、需要补写回日志区的文本 */
  leftover: string;
}

// 旧固件/模拟器：>=4 空格缩进的候选行
const CANDIDATE_LINE = /^ {4,}(\S.*)$/;
// 真机：`\r\n\t<候选>\0`，候选可能被日志插入打断，按 TAB+NUL 签名全局提取
const CANDIDATE_TAB = /\t+([^\s\0]+)\0/g;

function longestCommonPrefix(words: string[]): string {
  if (words.length === 0) return "";
  let prefix = words[0];
  for (const w of words.slice(1)) {
    while (prefix && !w.startsWith(prefix)) prefix = prefix.slice(0, -1);
    if (!prefix) return "";
  }
  return prefix;
}

/** 回显行里 `sentLine` 之后多出的词才算补全；`strict` 时整行只允许一个词 */
function completionToken(
  content: string,
  sentLine: string,
  sentToken: string,
  strict: boolean
): string | null {
  if (!content.startsWith(sentLine)) return null;
  const rest = content.slice(sentLine.length);
  if (!rest || rest.startsWith(" ")) return null;
  const m = (strict ? /^(\S+)\s*$/ : /^(\S+)(?=\s|$)/).exec(rest);
  return m ? sentToken + m[1] : null;
}

/**
 * 唯一匹配时设备不列候选，只回显补全结果：
 * - 旧固件：重绘提示符行 `~ # <已补全内容>`；
 * - 真机：无提示符，直接回显补全后的整词（可能带粘连日志）。
 */
function detectUniqueCompletion(
  lines: string[],
  sentLine: string,
  sentToken: string,
  prompt: string
): string | null {
  if (!sentLine) return null;
  for (const line of lines) {
    const at = line.lastIndexOf(prompt);
    if (at < 0) continue;
    const content = line.slice(at + prompt.length).split("\0")[0];
    const token = completionToken(content, sentLine, sentToken, false);
    if (token) return token;
  }
  for (const line of lines) {
    if (line.includes(prompt)) continue;
    const content = line.replace(/\0/g, "");
    // 严格整行匹配，避免把以同一前缀开头的日志误判成补全
    const token = completionToken(content, sentLine, sentToken, true);
    if (token) return token;
  }
  return null;
}

/**
 * 解析 Tab 查询窗口捕获到的原始输出。
 * 设备日志与补全输出交织，因此只依赖强特征：
 * 1. 候选行以 TAB（真机）或 >=4 空格（旧固件）开头，真机候选以 NUL 结束；
 * 2. 提示符重绘行含 `prompt`，NUL 分隔的第一个字段是行缓冲回显，其余是粘连日志；
 * 3. 其余文本一律视为日志，原样返回。
 */
export function parseCompletion(
  raw: string,
  sentLine: string,
  prompt: string = DEFAULT_PROMPT
): CompletionResult {
  const candidates: string[] = [];
  for (const m of raw.matchAll(CANDIDATE_TAB)) candidates.push(m[1]);

  const lines = raw.split("\n").map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  // nl 记录该行在原始输出里是否带换行，补写日志时保留行边界
  const rest: { text: string; nl: boolean }[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const noNul = line.replace(/\0+$/, "");
    const m = CANDIDATE_LINE.exec(noNul);
    if (m) {
      candidates.push(m[1].trimEnd());
      continue;
    }
    // TAB 候选行：行首字段是候选，NUL 之后可能粘连日志
    if (line.startsWith("\t")) {
      const fields = line.split("\0");
      if (fields.length > 1) {
        for (const f of fields.slice(1)) if (f.trim()) rest.push({ text: f, nl: false });
        continue;
      }
    }
    rest.push({ text: line, nl: i < lines.length - 1 });
  }

  const uniqueCandidates = [...new Set(candidates)];

  const tokenStart = sentLine.lastIndexOf(" ") + 1;
  const tokenPrefix = sentLine.slice(0, tokenStart);
  const sentToken = sentLine.slice(tokenStart);

  if (uniqueCandidates.length === 0) {
    const unique = detectUniqueCompletion(
      rest.map((r) => r.text),
      sentLine,
      sentToken,
      prompt
    );
    if (unique) uniqueCandidates.push(unique);
  }
  // 候选应补的是当前最后一个词；对不上说明不是这条命令的补全，整体丢弃
  if (uniqueCandidates.length && !uniqueCandidates.every((c) => c.startsWith(sentToken))) {
    uniqueCandidates.length = 0;
  }

  const tokenLcp = longestCommonPrefix(uniqueCandidates);
  // 对外的公共前缀是整行（参数补全时前半条命令要保留），不是单个词
  const lcp = uniqueCandidates.length ? tokenPrefix + tokenLcp : "";
  const remoteLine =
    uniqueCandidates.length === 0
      ? sentLine + "\t"
      : uniqueCandidates.length === 1
        ? tokenPrefix + uniqueCandidates[0] + " "
        : lcp;

  const sentEcho = sentLine.trimEnd();
  const remoteEcho = remoteLine.trimEnd();
  const leftoverLines: { text: string; nl: boolean }[] = [];
  const pushLeftover = (text: string, nl: boolean) => leftoverLines.push({ text, nl });
  for (const r of rest) {
    const line = r.text;
    // 行缓冲的大小写/后缀不参与判断，只丢与已知回显完全相同的行
    const lineText = line.replace(/\0/g, "").trimEnd();
    if (sentEcho && lineText === sentEcho) continue;
    if (remoteEcho && lineText === remoteEcho) continue;

    const at = line.lastIndexOf(prompt);
    if (at < 0) {
      if (line.includes("\0")) {
        for (const f of line.split("\0")) if (f.trim()) pushLeftover(f, false);
      } else {
        pushLeftover(line, r.nl);
      }
      continue;
    }
    // 重绘行：`~ # <行缓冲回显>\0<粘连日志>`；无 NUL 时为旧固件的连续文本
    const after = line.slice(at + prompt.length);
    const fields = after.split("\0");
    if (fields.length > 1) {
      for (const f of fields.slice(1)) {
        const t = f.trimEnd();
        if (t && t !== sentEcho && t !== remoteEcho) pushLeftover(f, false);
      }
    } else {
      let content = after;
      if (content.startsWith(remoteLine)) content = content.slice(remoteLine.length);
      if (content) pushLeftover(content, r.nl);
    }
  }

  // 候选列表前后的空行属于被下拉框替代的排版，避免每次 Tab 留下空行
  let start = 0;
  while (start < leftoverLines.length && leftoverLines[start].text.trim() === "") start++;
  let end = leftoverLines.length;
  while (end > start && leftoverLines[end - 1].text.trim() === "") end--;
  const kept = leftoverLines.slice(start, end);
  return {
    candidates: uniqueCandidates,
    lcp,
    remoteLine,
    leftover:
      kept.map((l) => l.text).join("\n") + (kept.length && kept[kept.length - 1].nl ? "\n" : ""),
  };
}

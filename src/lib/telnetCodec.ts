// Telnet 终端 GBK 编解码（RCU 固件中文输出为 GBK；ASCII 透明兼容 ANSI 转义序列）
// 码表由 scripts/gen-gbk-table.mjs 生成，索引：(首字节-0x81)*190 + (次字节<0x80 ? 次字节-0x40 : 次字节-0x41)
import gbkData from "./gbk-table.json";

const TABLE: number[] = gbkData.table;
const idxOf = (a: number, b: number) => (a - 0x81) * 190 + (b < 0x80 ? b - 0x40 : b - 0x41);

// 反向表：Unicode 码点 → 两字节 GBK（运行时一次性构建）
let reverse: Map<number, number> | null = null;
function reverseMap(): Map<number, number> {
  if (reverse) return reverse;
  reverse = new Map();
  for (let a = 0x81; a <= 0xfe; a++) {
    for (let b = 0x40; b <= 0xfe; b++) {
      if (b === 0x7f) continue;
      const cp = TABLE[idxOf(a, b)];
      if (cp && !reverse.has(cp)) reverse.set(cp, (a << 8) | b);
    }
  }
  return reverse;
}

/** 有状态 GBK 解码器：跨分包保留悬空首字节 */
export class GbkDecoder {
  private pending = 0; // 悬空的 GBK 首字节（0x81-0xFE），0 表示无

  /** 解码一段字节流为字符串（ASCII 原样透传） */
  push(bytes: Uint8Array): string {
    let out = "";
    for (let i = 0; i < bytes.length; i++) {
      const b = bytes[i];
      if (this.pending) {
        const lead = this.pending;
        this.pending = 0;
        if (b >= 0x40 && b <= 0xfe && b !== 0x7f) {
          const cp = TABLE[idxOf(lead, b)];
          out += cp ? String.fromCodePoint(cp) : "\uFFFD";
        } else {
          // 非法次字节：首字节按未映射处理，当前字节重新参与解析
          out += "\uFFFD";
          i--;
          continue;
        }
      } else if (b < 0x80) {
        out += String.fromCharCode(b);
      } else if (b === 0x80) {
        out += "\u20AC"; // GBK 单字节 0x80 → 欧元符
      } else {
        this.pending = b;
      }
    }
    return out;
  }

  reset() {
    this.pending = 0;
  }
}

/** 将输入字符串编码为 GBK 字节（ASCII 原样；无 GBK 映射的字符以 ? 替代） */
export function gbkEncode(text: string): Uint8Array {
  const rev = reverseMap();
  const out: number[] = [];
  for (const ch of text) {
    const cp = ch.codePointAt(0)!;
    if (cp < 0x80) {
      out.push(cp);
    } else {
      const g = rev.get(cp);
      if (g) {
        out.push(g >> 8, g & 0xff);
      } else {
        out.push(0x3f); // '?'
      }
    }
  }
  return Uint8Array.from(out);
}

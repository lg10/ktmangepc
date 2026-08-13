// 生成 GBK → Unicode 码表（Node TextDecoder 支持 gb18030，覆盖 GBK 区）
// 输出紧凑数组：idx = (first-0x81)*190 + (second<0x80 ? second-0x40 : second-0x41)，值为 Unicode 码点，未映射为 0
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dec = new TextDecoder("gb18030");
// 第二字节 0x40-0x7E 共 63 个 + 0x80-0xFE 共 127 个 = 190（跳过 0x7F）
const idxOf = (a, b) => (a - 0x81) * 190 + (b < 0x80 ? b - 0x40 : b - 0x41);
const table = new Array(126 * 190).fill(0);
let mapped = 0;
const buf = new Uint8Array(2);
for (let a = 0x81; a <= 0xfe; a++) {
  for (let b = 0x40; b <= 0xfe; b++) {
    if (b === 0x7f) continue;
    buf[0] = a;
    buf[1] = b;
    const s = dec.decode(buf);
    if (s && s !== "\uFFFD" && s.length === 1) {
      table[idxOf(a, b)] = s.codePointAt(0);
      mapped++;
    }
  }
}
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "src", "lib", "gbk-table.json");
writeFileSync(out, JSON.stringify({ idxNote: "(first-0x81)*190 + (second<0x80 ? second-0x40 : second-0x41)", table }));
console.log("mapped:", mapped, "->", out);

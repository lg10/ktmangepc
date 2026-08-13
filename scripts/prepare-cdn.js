#!/usr/bin/env node
/**
 * CDN 发版准备：合成/合并 latest.json + 按平台子目录改写 url + 产物重命名拷贝
 *
 * 用法：node scripts/prepare-cdn.js [searchRoot] [outDir] [baseUrl]
 *   默认：searchRoot=src-tauri/target  outDir=dist-cdn
 *         baseUrl=https://mange.kingint.com/app/kt-mange-pc
 *
 * latest.json 来源（按优先级）：
 *   1. CI（tauri-action）产物中已有的 latest.json → 直接合并
 *   2. 本地 tauri build 不产出 latest.json → 从 *.sig 更新包签名自行合成
 *
 * 产出 dist-cdn/（整个目录内容上传到 CDN 根路径即可）：
 *   latest.json
 *   mac/       kt-mange-pc-<arch>.app.tar.gz（更新包） / kt-mange-pc-<arch>.dmg（首装）
 *   windows/   kt-mange-pc-<arch>.nsis.zip（更新包） / kt-mange-pc-<arch>-setup.exe（首装）
 *   linux/     kt-mange-pc-<arch>.AppImage.tar.gz（更新包） / .AppImage / .deb（首装）
 */
import fs from "node:fs";
import path from "node:path";

const [searchRoot = "src-tauri/target", outDir = "dist-cdn", base = "https://mange.kingint.com/app/kt-mange-pc"] =
  process.argv.slice(2);

const DIR_OF = (key) =>
  key.startsWith("darwin") ? "mac" : key.startsWith("windows") ? "windows" : key.startsWith("linux") ? "linux" : null;
const SUBDIR_OF = (dir) => (dir === "mac" ? "macos" : dir === "windows" ? "nsis" : "appimage");

// rust target triple → updater 平台 key
const TRIPLE_TO_KEY = {
  "aarch64-apple-darwin": "darwin-aarch64",
  "x86_64-apple-darwin": "darwin-x86_64",
  "x86_64-pc-windows-msvc": "windows-x86_64",
  "i686-pc-windows-msvc": "windows-i686",
  "x86_64-unknown-linux-gnu": "linux-x86_64",
};

const merged = { version: "", notes: "", pub_date: "", platforms: {} };
const bundleRootOf = {}; // platformKey -> .../release/bundle 目录

// ---------- 路径 1：递归找 latest.json（CI 产物，跳过 debug） ----------
function findLatestJsons(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (e.name !== "debug") findLatestJsons(p, out);
    } else if (e.name === "latest.json") out.push(p);
  }
  return out;
}

for (const f of findLatestJsons(path.resolve(searchRoot))) {
  const j = JSON.parse(fs.readFileSync(f, "utf8"));
  if (j.version) merged.version = j.version;
  if (j.notes) merged.notes = j.notes;
  if (j.pub_date) merged.pub_date = j.pub_date;
  for (const [k, v] of Object.entries(j.platforms || {})) {
    merged.platforms[k] = v;
    const d = path.dirname(f);
    bundleRootOf[k] = ["macos", "nsis", "appimage"].includes(path.basename(d)) ? path.dirname(d) : d;
  }
}

// ---------- 路径 2：本地无 latest.json → 从 .sig 合成 ----------
// 递归扫描（CI 下载产物为 bundles/bundle-<triple>/{macos,nsis,appimage}/，
// 本地为 <triple>/release/bundle/...，两种布局都兼容）
function synthFromSig(dir) {
  if (!fs.existsSync(dir)) return;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!e.isDirectory()) continue;
    const p = path.join(dir, e.name);
    const key = TRIPLE_TO_KEY[e.name] || TRIPLE_TO_KEY[e.name.replace(/^(bundle|KT Device Scan|KT_Device_Scan)-/, "")];
    if (key && !merged.platforms[key] && tryCollect(key, p)) continue;
    synthFromSig(p);
  }
}

function tryCollect(key, dir) {
  // dir 本身即 bundle 根（CI 布局）或 dir/release/bundle（本地布局）
  return collectPlatform(key, dir) || collectPlatform(key, path.join(dir, "release", "bundle"));
}

function collectPlatform(key, bundleRoot) {
  const dir = DIR_OF(key);
  const artDir = path.join(bundleRoot, SUBDIR_OF(dir));
  if (!fs.existsSync(artDir)) return false;
  const sig = fs.readdirSync(artDir).find((n) => n.endsWith(".sig"));
  if (!sig) return false;
  merged.platforms[key] = {
    signature: fs.readFileSync(path.join(artDir, sig), "utf8").trim(),
    url: sig.slice(0, -4), // 去掉 .sig 即更新包文件名（占位，后面会改写）
    version: merged.version,
  };
  bundleRootOf[key] = bundleRoot;
  return true;
}

if (!Object.keys(merged.platforms).length) {
  synthFromSig(path.resolve(searchRoot));
  // 版本取自 tauri.conf.json，发布时间取当前
  const conf = JSON.parse(fs.readFileSync(path.resolve("src-tauri/tauri.conf.json"), "utf8"));
  merged.version = conf.version || merged.version;
  for (const p of Object.values(merged.platforms)) p.version = merged.version;
  merged.pub_date = new Date().toISOString();
}

if (!Object.keys(merged.platforms).length) {
  console.error("[error] 未找到任何 latest.json 或 .sig 更新包，先执行打包");
  process.exit(1);
}

// 已知更新包后缀（保持与 Tauri 生成一致）
const UPDATER_EXT = [".app.tar.gz", ".nsis.zip", ".AppImage.tar.gz"];

function copyIfExist(src, dest) {
  if (src && fs.existsSync(src)) {
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(src, dest);
    return true;
  }
  return false;
}

function firstGlob(dir, pred) {
  if (!fs.existsSync(dir)) return null;
  const hit = fs.readdirSync(dir).filter(pred);
  return hit.length ? path.join(dir, hit[0]) : null;
}

for (const [key, plat] of Object.entries(merged.platforms)) {
  const dir = DIR_OF(key);
  const arch = key.split("-")[1] || "x86_64";
  if (!dir) continue;

  const oldName = path.basename(plat.url || "");
  const ext = UPDATER_EXT.find((x) => oldName.endsWith(x)) || ".bin";
  const newName = `kt-mange-pc-${arch}${ext}`;
  plat.url = `${base}/${dir}/${newName}`;

  const bundleRoot = bundleRootOf[key];
  if (!bundleRoot) continue;

  // 更新包：按旧文件名定位后重命名拷贝
  copyIfExist(path.join(bundleRoot, SUBDIR_OF(dir), oldName), path.join(outDir, dir, newName));

  // 首装包
  if (dir === "mac") {
    const dmg = firstGlob(path.join(bundleRoot, "dmg"), (n) => n.endsWith(".dmg"));
    copyIfExist(dmg, path.join(outDir, dir, `kt-mange-pc-${arch}.dmg`));
  } else if (dir === "windows") {
    const exe = firstGlob(path.join(bundleRoot, "nsis"), (n) => n.endsWith("-setup.exe"));
    copyIfExist(exe, path.join(outDir, dir, `kt-mange-pc-${arch}-setup.exe`));
  } else {
    const app = firstGlob(path.join(bundleRoot, "appimage"), (n) => n.endsWith(".AppImage"));
    copyIfExist(app, path.join(outDir, dir, `kt-mange-pc-${arch}.AppImage`));
    const deb = firstGlob(path.join(bundleRoot, "deb"), (n) => n.endsWith(".deb"));
    copyIfExist(deb, path.join(outDir, dir, `kt-mange-pc-${arch}.deb`));
  }
}

fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "latest.json"), JSON.stringify(merged, null, 2));

console.log(`[done] ${outDir}/ 已生成，platforms: ${Object.keys(merged.platforms).join(", ")}`);
console.log("上传映射（dist-cdn/ 内容 → CDN 根路径）：");
for (const [k, p] of Object.entries(merged.platforms)) console.log(`  ${k}: ${p.url}`);

#!/usr/bin/env node
/**
 * 肯天玉佩自建更新服务端（适配 Tauri Updater v2）
 *
 * 用法：
 *   1. 发布新版本：pnpm tauri build 产出安装包与 .sig 签名文件
 *   2. 将安装包上传到可公网访问的静态服务（OSS/S3/nginx 均可），记录 URL
 *   3. 编辑 releases.json（见同目录示例），node server.js 启动（默认 8787 之外的 9123 端口）
 *
 * Tauri Updater 客户端请求：GET {endpoint}/{target}/{arch}/{currentVersion}
 * 返回 204 表示无更新；返回 JSON 表示有更新：
 *   { "version": "3.1.0", "notes": "...", "pub_date": "ISO8601",
 *     "url": "https://cdn.../app.dmg", "signature": "<tauri signer 生成的 .sig 内容>" }
 *
 * 签名生成（构建机）：
 *   pnpm tauri signer generate -w ~/.tauri/kt-updater.key
 *   pnpm tauri build -- --signer-path ~/.tauri/kt-updater.key  (或 tauri signer sign)
 * 客户端公钥配置在 src-tauri/tauri.conf.json 的 plugins.updater.pubkey。
 */
const http = require("http");
const fs = require("fs");
const path = require("path");

const PORT = Number(process.env.PORT || 9123);
const RELEASES_FILE = path.join(__dirname, "releases.json");

/**
 * releases.json 结构示例：
 * {
 *   "latest": {
 *     "darwin-aarch64": { "version": "3.0.0", "notes": "首版 Tauri 迁移", "url": "https://cdn.kingint.com/kt/3.0.0/app.dmg", "signatureFile": "app.dmg.sig" },
 *     "windows-x86_64": { "version": "3.0.0", "notes": "首版 Tauri 迁移", "url": "https://cdn.kingint.com/kt/3.0.0/app.msi", "signatureFile": "app.msi.sig" },
 *     "linux-x86_64":  { "version": "3.0.0", "notes": "首版 Tauri 迁移", "url": "https://cdn.kingint.com/kt/3.0.0/app.AppImage", "signatureFile": "app.AppImage.sig" }
 *   }
 * }
 */
function loadReleases() {
  try {
    return JSON.parse(fs.readFileSync(RELEASES_FILE, "utf8"));
  } catch {
    return { latest: {} };
  }
}

/** 语义化版本比较：1 => a 更新 */
function cmpVer(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] || 0) > (pb[i] || 0)) return 1;
    if ((pa[i] || 0) < (pb[i] || 0)) return -1;
  }
  return 0;
}

http
  .createServer((req, res) => {
    const m = req.url.match(/^\/update\/([^/]+)\/([^/]+)\/([^/?]+)/);
    if (!m) {
      res.writeHead(404).end();
      return;
    }
    const [, target, arch, current] = m;
    const key = `${target}-${arch}`;
    const rel = loadReleases().latest[key];
    if (!rel || cmpVer(rel.version, current) <= 0) {
      res.writeHead(204).end(); // 无更新
      return;
    }
    let signature = "";
    try {
      signature = fs
        .readFileSync(path.join(__dirname, rel.signatureFile), "utf8")
        .trim();
    } catch {
      /* 签名缺失时返回空串，客户端会校验失败并提示 */
    }
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        version: rel.version,
        notes: rel.notes || "",
        pub_date: new Date().toISOString(),
        url: rel.url,
        signature,
      })
    );
  })
  .listen(PORT, () => {
    console.log(`[kt-updater] 自建更新服务已启动: http://0.0.0.0:${PORT}/update/{target}/{arch}/{version}`);
  });

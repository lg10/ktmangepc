#!/usr/bin/env bash
# macOS 签名打包（Developer ID 签名 + 可选公证）
#
# 用法：
#   pnpm build:release:mac:signed                                # 仅签名
#   APPLE_ID=邮箱 APPLE_PASSWORD=App专用密码 pnpm build:release:mac:signed   # 签名+公证
#
# 证书：src-tauri/keys/developer-id.p12（已 gitignore，严禁提交）
set -e
cd "$(dirname "$0")/.."

# 本地凭据文件（已 gitignore）：APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID
if [ -f .env.apple ]; then
  set -a
  # shellcheck disable=SC1091
  . ./.env.apple
  set +a
fi
# 空值视为未设置（Tauri 对环境变量“存在即启用公证”，空用户名会 401）
[ -n "$APPLE_ID" ] || unset APPLE_ID
[ -n "$APPLE_PASSWORD" ] || unset APPLE_PASSWORD
[ -n "$APPLE_TEAM_ID" ] || unset APPLE_TEAM_ID

export APPLE_CERTIFICATE="$(openssl base64 -A -in src-tauri/keys/developer-id.p12)"
export APPLE_CERTIFICATE_PASSWORD="${APPLE_CERTIFICATE_PASSWORD:-0000}"
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Kentian (Chengdu) Information Technology Co., Ltd. (ZLLP55LM3V)}"
export APPLE_TEAM_ID="${APPLE_TEAM_ID:-ZLLP55LM3V}"
# updater 更新包签名私钥（Tauri 只认内容变量 TAURI_SIGNING_PRIVATE_KEY）
export TAURI_SIGNING_PRIVATE_KEY="$(cat src-tauri/keys/kt-mange-pc.key)"
export TAURI_SIGNING_PRIVATE_KEY_PATH=src-tauri/keys/kt-mange-pc.key
# 私钥为空密码生成，显式给空值避免交互提示卡住后台构建
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

if [ -n "$APPLE_ID" ] && [ -n "$APPLE_PASSWORD" ]; then
  echo "[info] 已启用公证（notarize + staple）"
else
  echo "[warn] 未设置 APPLE_ID/APPLE_PASSWORD：本次仅签名不公证"
fi

# 内置 adb 工具预签名：公证要求嵌套二进制带 Developer ID 签名与 hardened runtime。
# 先暂存到 resources/adb 并写入平台标记，build.rs 见标记同平台时跳过重拷，签名得以保留。
ADB_RES=src-tauri/resources/adb
if [ ! -d "$ADB_RES" ]; then
  mkdir -p src-tauri/resources
  cp -R src-tauri/vendor/adb/platform-tools-mac "$ADB_RES"
fi
printf 'platform-tools-mac' > src-tauri/resources/.adb-staged
for bin in adb etc1tool fastboot hprof-conv make_f2fs make_f2fs_casefold mke2fs sqlite3; do
  if [ -f "$ADB_RES/$bin" ]; then
    echo "[info] 签名内置工具: $bin"
    codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$ADB_RES/$bin"
  fi
done

pnpm tauri build --target aarch64-apple-darwin
pnpm tauri build --target x86_64-apple-darwin

# Tauri 只 staple app，dmg 需单独公证+staple（离线安装免 Gatekeeper 联网校验）
if [ -n "$APPLE_ID" ] && [ -n "$APPLE_PASSWORD" ]; then
  for dmg in src-tauri/target/*-apple-darwin/release/bundle/dmg/*.dmg; do
    echo "[info] 公证 dmg: $dmg"
    xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
    xcrun stapler staple "$dmg"
  done
fi

echo "[done] 产物位于 src-tauri/target/{aarch64,x86_64}-apple-darwin/release/bundle/"

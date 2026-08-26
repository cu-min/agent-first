#!/bin/bash
# Agent-first 一键更新
# 用法：改完代码（后端 Rust 或前端 web/src）后，在项目根目录运行 ./update.sh
# 作用：重新构建后端 release + 前端 dist，并重启 launchd 常驻服务
set -e
cd "$(dirname "$0")"

echo "== [1/3] 构建后端 (cargo build --release) =="
( cd server && cargo build --release ) || { echo "❌ 后端构建失败"; exit 1; }

echo "== [2/3] 构建前端 (npm run build) =="
( cd web && npm run build ) || { echo "❌ 前端构建失败"; exit 1; }

echo "== [3/3] 重启常驻服务 (launchd) =="
launchctl kickstart -k "gui/$(id -u)/com.tiklab.agentfirst.server" || { echo "❌ 服务重启失败"; exit 1; }
sleep 2

echo "== 验证 =="
curl -s -m 5 http://127.0.0.1:8080/healthz && echo
echo "✅ 更新完成，服务已用新代码重启。"

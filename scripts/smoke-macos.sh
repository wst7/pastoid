#!/usr/bin/env bash
# macOS 冒烟测试：启动 app，用 selftest feature 连续开合面板 40 次，
# 确认进程没崩、没 panic、没产生新的 crash report。
#
# 只在 macOS 上有意义（测的是 NSPanel 生命周期），其他平台直接跳过。
set -uo pipefail
cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Darwin" ]; then
  echo "⏭  非 macOS，跳过"
  exit 0
fi

LOG=/tmp/pastoid-smoke.log
STAMP=/tmp/pastoid-smoke.stamp
rm -f "$LOG"
touch "$STAMP"

cleanup() {
  pkill -f "target/debug/pastoid" 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT

echo "▶ 启动 dev app（selftest feature）…"
PASTOID_PANEL_SELFTEST=1 npm run tauri dev -- --features selftest >"$LOG" 2>&1 &
DEV_PID=$!

for _ in $(seq 1 120); do
  grep -q "selftest done" "$LOG" && break
  kill -0 "$DEV_PID" 2>/dev/null || break
  sleep 2
done

if ! grep -q "selftest done" "$LOG"; then
  echo "❌ selftest 没有跑完"
  tail -20 "$LOG"
  exit 1
fi

if grep -qi "panic" "$LOG"; then
  echo "❌ 日志里出现 panic"
  grep -i -A5 "panic" "$LOG" | head -20
  exit 1
fi

sleep 2
CRASH=$(find "$HOME/Library/Logs/DiagnosticReports" -name "pastoid-*.ips" -newer "$STAMP" 2>/dev/null | head -1)
if [ -n "$CRASH" ]; then
  echo "❌ 产生了崩溃报告: $CRASH"
  exit 1
fi

if ! grep -q "selftest toggle 39" "$LOG"; then
  echo "❌ 开合次数不足"
  tail -20 "$LOG"
  exit 1
fi

echo "✅ 面板开合 40 次：无崩溃、无 panic、无新 crash report"

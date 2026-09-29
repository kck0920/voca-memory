#!/usr/bin/env bash
# Voca Memory 리눅스 데스크톱 앱 등록 해제 스크립트.
set -euo pipefail

APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/voca-memory"

echo "Voca Memory 데스크톱 등록을 해제합니다..."

rm -f "$APPS_DIR/voca-memory.desktop"
rm -f "$ICONS_BASE/scalable/apps/voca-memory.svg"
rm -f "$ICONS_BASE/128x128/apps/voca-memory.png"
rm -f "$ICONS_BASE/64x64/apps/voca-memory.png"
rm -f "$ICONS_BASE/48x48/apps/voca-memory.png"
rm -f "$ICONS_BASE/32x32/apps/voca-memory.png"
rm -f "$ICONS_BASE/16x16/apps/voca-memory.png"

if [ -f "$DATA_DIR/server.pid" ]; then
  PID=$(cat "$DATA_DIR/server.pid" 2>/dev/null || true)
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    echo "실행 중인 Voca Memory 백그라운드 서버를 종료합니다 (PID: $PID)..."
    kill "$PID" || true
  fi
  rm -f "$DATA_DIR/server.pid"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS_DIR" || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICONS_BASE" || true
fi

echo "✓ Voca Memory 등록이 해제되었습니다."

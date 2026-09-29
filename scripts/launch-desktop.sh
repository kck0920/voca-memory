#!/usr/bin/env bash
# Voca Memory 데스크톱 앱 런처.
# 기본값으로 공식 배포 서버(https://voca-memory.duckdns.org)와 연동합니다.
# 로컬 개발/오프라인 모드는 --local 옵션 또는 VOCA_APP_URL 로 지정할 수 있습니다.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DEFAULT_REMOTE_URL="https://voca-memory.duckdns.org"
TARGET_URL="${VOCA_APP_URL:-$DEFAULT_REMOTE_URL}"

USE_LOCAL=0
BROWSER_ARGS=()

for arg in "$@"; do
  if [ "$arg" = "--local" ]; then
    USE_LOCAL=1
  else
    BROWSER_ARGS+=("$arg")
  fi
done

if [ "${VOCA_LOCAL_ONLY:-0}" = "1" ]; then
  USE_LOCAL=1
fi

if [ "$USE_LOCAL" = "1" ]; then
  DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/voca-memory"
  mkdir -p "$DATA_DIR"

  DB_PATH="${VOCA_DB_PATH:-$DATA_DIR/voca.db}"
  PORT="${VOCA_PORT:-3000}"
  HOST="${VOCA_HOST:-127.0.0.1}"
  TARGET_URL="http://${HOST}:${PORT}"

  # 로컬 서버 헬스체크 및 기동
  is_local_server_ready() {
    curl -s --max-time 1 "${TARGET_URL}/api/health" 2>/dev/null | grep -q "ok"
  }

  if ! is_local_server_ready; then
    SERVER_BIN=""
    if [ -x "$PROJECT_ROOT/target/release/voca-server" ]; then
      SERVER_BIN="$PROJECT_ROOT/target/release/voca-server"
    elif [ -x "$PROJECT_ROOT/target/debug/voca-server" ]; then
      SERVER_BIN="$PROJECT_ROOT/target/debug/voca-server"
    else
      echo "voca-server 바이너리를 빌드합니다..."
      (cd "$PROJECT_ROOT" && cargo build -p voca-server)
      SERVER_BIN="$PROJECT_ROOT/target/debug/voca-server"
    fi

    export VOCA_DB_PATH="$DB_PATH"
    export VOCA_HOST="$HOST"
    export VOCA_PORT="$PORT"
    export VOCA_ALLOWED_ORIGINS="http://${HOST}:${PORT},http://localhost:${PORT}"
    export VOCA_SECURE_COOKIES=0

    echo "Voca Memory 로컬 백엔드 서버를 기동합니다 ($TARGET_URL)..."
    nohup "$SERVER_BIN" > "$DATA_DIR/server.log" 2>&1 &
    echo $! > "$DATA_DIR/server.pid"

    for _ in {1..25}; do
      if is_local_server_ready; then
        break
      fi
      sleep 0.2
    done
  fi
else
  # 원격 서버 헬스체크 (네트워크 상태 확인)
  if ! curl -s --max-time 3 "${TARGET_URL}/api/health" 2>/dev/null | grep -q "ok"; then
    echo "알림: 원격 서버 ($TARGET_URL) 상태를 확인하세요." >&2
    if command -v notify-send >/dev/null 2>&1; then
      notify-send "Voca Memory" "원격 서버 ($TARGET_URL) 에 연결할 수 없습니다. 네트워크를 확인하세요." 2>/dev/null || true
    fi
  fi
fi

# 브라우저 전용 앱 모드(--app) 실행
for browser in chromium google-chrome brave-browser microsoft-edge; do
  if command -v "$browser" >/dev/null 2>&1; then
    exec "$browser" --app="$TARGET_URL" --class=voca-memory.duckdns.org "${BROWSER_ARGS[@]}"
  fi
done

if command -v firefox >/dev/null 2>&1; then
  exec firefox --new-window "$TARGET_URL" "${BROWSER_ARGS[@]}"
fi

if command -v xdg-open >/dev/null 2>&1; then
  exec xdg-open "$TARGET_URL"
fi

echo "오류: 지원되는 웹 브라우저를 찾을 수 없습니다." >&2
exit 1

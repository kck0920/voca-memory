#!/usr/bin/env bash
# Voca Memory 리눅스 데스크톱 앱 등록 스크립트.
# 시스템 메뉴 및 런처에 'Voca Memory' 앱과 아이콘을 등록합니다.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"

echo "── 1. 아이콘 설치 중 ──"
mkdir -p "$ICONS_BASE/scalable/apps"
mkdir -p "$ICONS_BASE/128x128/apps"
mkdir -p "$ICONS_BASE/64x64/apps"
mkdir -p "$ICONS_BASE/48x48/apps"
mkdir -p "$ICONS_BASE/32x32/apps"
mkdir -p "$ICONS_BASE/16x16/apps"

cp "$PROJECT_ROOT/crates/voca-ui/assets/icon.svg" "$ICONS_BASE/scalable/apps/voca-memory.svg"
cp "$PROJECT_ROOT/crates/voca-ui/assets/icon-128x128.png" "$ICONS_BASE/128x128/apps/voca-memory.png"
cp "$PROJECT_ROOT/crates/voca-ui/assets/icon-64x64.png" "$ICONS_BASE/64x64/apps/voca-memory.png"
cp "$PROJECT_ROOT/crates/voca-ui/assets/icon-48x48.png" "$ICONS_BASE/48x48/apps/voca-memory.png"
cp "$PROJECT_ROOT/crates/voca-ui/assets/favicon-32x32.png" "$ICONS_BASE/32x32/apps/voca-memory.png"
cp "$PROJECT_ROOT/crates/voca-ui/assets/favicon-16x16.png" "$ICONS_BASE/16x16/apps/voca-memory.png"

echo "아이콘이 $ICONS_BASE 에 설치되었습니다."

echo "── 2. 런처 스크립트 권한 확인 ──"
chmod +x "$PROJECT_ROOT/scripts/launch-desktop.sh"

echo "── 3. .desktop 파일 생성 중 ──"
mkdir -p "$APPS_DIR"
DESKTOP_FILE="$APPS_DIR/voca-memory.desktop"

cat << EOF > "$DESKTOP_FILE"
[Desktop Entry]
Version=1.0
Type=Application
Name=Voca Memory
GenericName=Vocabulary Review Terminal
Comment=영어 어휘장 · 복습 터미널 (FSRS Spaced Repetition)
Exec=$PROJECT_ROOT/scripts/launch-desktop.sh %U
Icon=voca-memory
Terminal=false
Categories=Education;Languages;
StartupNotify=true
StartupWMClass=voca-memory.duckdns.org
Keywords=vocabulary;flashcard;memory;english;voca;단어;영어;
EOF

chmod +x "$DESKTOP_FILE"
echo "데스크톱 엔트리가 등록되었습니다: $DESKTOP_FILE"

echo "── 4. 시스템 데스크톱/아이콘 캐시 갱신 ──"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS_DIR" || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICONS_BASE" || true
fi

echo "── 5. 서버 바이너리 확인 ──"
if [ ! -x "$PROJECT_ROOT/target/debug/voca-server" ] && [ ! -x "$PROJECT_ROOT/target/release/voca-server" ]; then
  echo "voca-server 바이너리를 빌드합니다..."
  (cd "$PROJECT_ROOT" && cargo build -p voca-server)
fi

echo "✓ Voca Memory 앱 등록이 완료되었습니다!"
echo "  - 이제 애플리케이션 메뉴나 검색(KRunner/GNOME)에서 'Voca Memory'를 실행할 수 있습니다."
echo "  - 터미널에서 즉시 실행해 보려면: $PROJECT_ROOT/scripts/launch-desktop.sh"

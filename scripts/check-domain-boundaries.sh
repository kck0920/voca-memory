#!/usr/bin/env bash
# ADR-0003 가드. 이 스크립트가 실패하면 서버·브라우저 양쪽에서 도메인을 돌릴 수 없다.
#
# 두 가지를 지킨다:
#   1. voca-domain이 허용 밖 크레이트를 직접 의존하지 않는가
#   2. voca-domain이 wasm32-unknown-unknown에서 실제로 컴파일되는가
#
# 두 번째 검사가 없으면 rayon이나 getrandom 같은 의존이 슬그슬 추가되어도
# 브라우저에서 깨지는 걸 CI가 잡지 못한다. 실제로 getrandom이 이 방식을 막은 바 있다
# (docs/adr/0008-getrandom-wasm-shim.md).
set -euo pipefail

cd "$(dirname "$0")/.."

# `--edges normal`로 dev-dependency를 제외한다. dev-dep은 Wasm 번들에 들어가지 않으므로
# 규칙 대상이 아니다. 그리고 직접 의존성만 본다 — fsrs가 rayon·ndarray를 끌어오는 건
# fsrs의 문제지 우리 의존 규칙의 문제가 아니다. 그 tree는 fsrs를 빼면 통째로 사라진다.
ALLOWED='^(fsrs|serde|serde_derive|time|uuid|getrandom) v'
STATUS=0

echo "── voca-domain 직접 의존성 allowlist 검사 ──"
UNEXPECTED=$(cargo tree -p voca-domain --edges normal --depth 1 --prefix none 2>/dev/null \
  | tail -n +2 \
  | grep -vE "$ALLOWED" || true)

if [ -n "$UNEXPECTED" ]; then
  echo "FAIL: 허용되지 않은 직접 의존성이 있다"
  echo "$UNEXPECTED" | sed 's/^/  - /'
  echo
  echo "새 의존이 필요하다면 docs/design.md의 의존 규칙 표를 먼저 고쳐라."
  echo "그 전에 voca-domain이 서버·브라우저 양쪽에서 돌 수 있는지 확인해야 한다."
  STATUS=1
else
  echo "OK"
  cargo tree -p voca-domain --edges normal --depth 1 --prefix none 2>/dev/null \
    | tail -n +2 | sed 's/^/  /'
fi

echo
echo "── voca-domain wasm32 컴파일 검사 ──"
if cargo check -p voca-domain --target wasm32-unknown-unknown -q 2>/tmp/voca-wasm-check.log; then
  echo "OK"
else
  echo "FAIL: wasm32에서 컴파일되지 않는다"
  sed 's/^/  /' /tmp/voca-wasm-check.log
  STATUS=1
fi

echo
echo "── 공개 API에 async가 새어들었는지 검사 ──"
# async가 하나라도 노출되면 voca-domain은 브라우저에서 그대로 못 쓴다.
if grep -rn --include='*.rs' -E '^\s*pub (async )?fn [a-z_]+\([^)]*\)\s*->\s*impl .*Future|pub async fn' crates/voca-domain/src/; then
  echo "FAIL: 공개 API에 async 함수가 있다"
  STATUS=1
else
  echo "OK"
fi

exit $STATUS

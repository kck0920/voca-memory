#!/usr/bin/env bash
# 라이브 스모크. 서버를 띄우고 한 바퀴 돌린 뒤 닫는다.
set -u
cd "$(dirname "$0")/.."
PORT="${1:-8971}"
BIN=target/debug/voca-server
DB=$(mktemp -d)/v.db
JAR=$(mktemp)
ORIGIN="https://voca.example.kr"
BASE="http://127.0.0.1:$PORT"

VOCA_PORT="$PORT" VOCA_DB_PATH="$DB" VOCA_ALLOWED_ORIGINS="$ORIGIN" \
  "$BIN" > /tmp/smoke-server.log 2>&1 &
SRV=$!
trap 'kill "$SRV" 2>/dev/null' EXIT

for _ in $(seq 1 40); do
  curl -sf -m 2 "$BASE/api/health" >/dev/null 2>&1 && break
  sleep 0.5
done

api() { curl -s -m 10 -b "$JAR" -c "$JAR" -H "Origin: $ORIGIN" -H 'Content-Type: application/json' "$@"; }
jq_() { python3 -c "import json,sys;d=json.load(sys.stdin);print($1)"; }

echo "=== 1. 등록 ==="
api -X POST "$BASE/api/auth/register" \
  -d '{"email":"live@b.kr","password":"부지런한-비밀번호","display_name":"라이브","timezone":"Asia/Seoul","retention":null}' | jq_ "d['user_id']"

echo "=== 2. 덱 만들기 ==="
DECK=$(api -X POST "$BASE/api/decks" -d '{"name":"토익 3000"}')
echo "$DECK" | jq_ "d['name'] + ' (신규/일 ' + str(d['new_per_day']) + ')'"
DECK_ID=$(echo "$DECK" | jq_ "d['deck_id']")

echo "=== 3. 내 뜻 추가 ==="
S=$(api -X POST "$BASE/api/senses" \
  -d '{"lemma":"run","kind":"word","definition":"달리다","pos":"verb","example_en":null}')
SID=$(echo "$S" | jq_ "d['sense_id']")
echo "sense=$SID kind=$(echo "$S" | jq_ "d['kind']")"

echo "=== 4. 덱에 Card 로 추가 ==="
api -X POST "$BASE/api/decks/cards" -d "{\"deck_id\":\"$DECK_ID\",\"sense_ids\":[\"$SID\"]}" | jq_ "str(len(d)) + '장'"

echo "=== 5. 복습 큐 (Card Front) ==="
Q=$(api "$BASE/api/study/queue")
echo "$Q" | jq_ "d['cards'][0]['front']['kind'] + ': ' + d['cards'][0]['front']['text'] + ' → ' + d['cards'][0]['definition']"
CARD=$(echo "$Q" | jq_ "d['cards'][0]['card_id']")

echo "=== 6. Good 평가 ==="
R=$(api -X POST "$BASE/api/study/review" -d "{\"card_id\":\"$CARD\",\"rating\":\"good\"}")
echo "$R" | jq_ "f\"XP {d['xp_earned']} · {d['streak']['current']}일 연속 · Lv{d['level']['level']} · 다음 복습 {d['due_at'][:10]}\""
echo "$R" | jq_ "'버튼: ' + ' / '.join(d['preview'][r]['label'] for r in ['again','hard','good','easy'])"

echo "=== 7. 대시보드 ==="
api "$BASE/api/dashboard" | jq_ "'복습할 ' + str(d['reviews_due']) + '장 · 새 ' + str(d['new_remaining']) + '장 · 위험 ' + str(d['streak_at_risk'])"

echo "=== 8. 로그인한 SSR 페이지 ==="
api "$BASE/" | grep -oE 'class="streak">[^<]*' | head -1

echo "=== 9. 다시 평가하면 큐가 비었는가 ==="
api "$BASE/api/study/queue" | jq_ "str(len(d['cards'])) + '장 남음'"

echo "=== 10. 사전 조회 (외부 네트워크 없음) ==="
api "$BASE/api/dict/lookup?lemma=abandon" | jq_ "d['status']"

echo "=== 11. 동기화 (다른 기기가 볼 타임라인) ==="
api "$BASE/api/sync/changes?since=0" | jq_ "'변경 ' + str(len(d['changes'])) + '건 · watermark ' + str(d['watermark'])"
api "$BASE/api/sync/changes?since=0" | jq_ "'Card 스케줄러: ' + str([c for c in d['changes'] if c['kind']=='card'][0]['item']['memory']['state'])"

echo "=== 12. CSRF: 외부 Origin 의 변경 요청 ==="
curl -s -m 10 -b "$JAR" -o /dev/null -w "%{http_code}\n" -X POST "$BASE/api/study/review" \
  -H 'Origin: https://evil.example.com' -H 'Content-Type: application/json' \
  -d "{\"card_id\":\"$CARD\",\"rating\":\"good\"}"

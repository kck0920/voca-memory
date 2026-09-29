# Voca Memory 사용법

영어 어휘를 **뜻(Sense) 단위**로 복습하고, FSRS-6 스케줄러로 다음 복습 시점을
계산해 주는 웹 앱이다. 설계 근거는 [`docs/design.md`](docs/design.md), 용어는
[`CONTEXT.md`](CONTEXT.md).

> **현재 상태를 먼저 읽을 것.** 브라우저 화면은 아직 대시보드 헤더만 그린다.
> 복습 버튼·로그인 폼·덱 관리 화면은 **아직 없다.** 지금 실제로 쓸 수 있는 경로는
> **HTTP API** 다. 아래 [브라우저에서 직접 쓰기](#브라우저에서-직접-쓰기) 와
> [API 로 쓰기](#api-로-쓰기) 중 후자가 현재 유일하게 완전한 흐름이다.
> [요약](#요약-한-눈에-보기) 에 상태를 표로 정리했다.

---

## 요약: 한눈에 보기

| 기능 | 상태 | 경로 |
|---|---|---|
| 회원가입 · 로그인 · 로그아웃 | 동작 | `POST /api/auth/*`, 브라우저 상단/로그인 폼 |
| 대시보드 (Streak · XP · Level) | 동작 | `GET /api/dashboard`, `GET /` |
| 내 뜻 추가 · 덱 생성 · Card 추가 | 동작 | `POST /api/senses`, `/api/decks`, `/api/decks/cards`, UI 탭 |
| 복습 큐 · 4버튼 평가 | 동작 | `GET /api/study/queue`, `POST /api/study/review`, 브라우저 플래시카드 화면 |
| 외부 사전 조회 (dictionaryapi.dev) | 동작 | `GET /api/dict/lookup`, UI 사전 검색 |
| 기본 시드 단어 (핵심 다의어 30선) | 동작 | 서버 시작 시 자동 적재, `POST /api/decks/seed`, UI 원클릭 가져오기 |
| 브라우저 복습 화면 (4버튼) | 동작 | 플래시카드 앞/뒷면 뒤집기(Space), 4개 평가 버튼 및 키보드 단축키(1~4) |
| 브라우저 로그인 폼 | 동작 | 로그인 / 회원가입 탭 폼 제공 |
| Retention Preset 변경 화면 | 동작 | 학습 설정 탭에 `RetentionPicker` 마운트 |
| CSS (스타일시트) & 스크립트 | 동작 | `retro.css` 와 `app.js` 가 `render_page` 에 인라인 번들 |
| 단어 검색 화면 · 사용자 Sense 편집 | 동작 | 실시간 사전 검색 결과에서 덱에 추가, 직접 뜻 등록 폼 |

---

## 핵심 개념

여기서부터 용어가 갈린다. 코드를 열기 전에 이 4개만 구분해 두면 된다.

- **Word** (어 항목) — 표기형(`run`) 하나. 사전 단위.
- **Sense** (뜻) — Word가 가지는 **뜻 하나**. `run`의 "달리다"와 "운영하다"는
  **별개의 Sense** 다. 이 앱의 복습 단위는 Word가 아니라 **Sense** 다. 이게 이 앱의
  핵심이다 — 다의어를 뜻별로 따로 외운다.
- **Card** (학습 단위) — Sense 하나를 향하는 복습 단위. **정확히 하나의 Sense**,
  **정확히 하나의 덱**에 속한다.
- **Deck** (덱) — Card의 묶음. 하나의 학습 목표(예: "토익 3000").

하나의 Word에 여러 Card가 생긴다. `run`을 세 가지 뜻으로 넣으면 Card가 세 장이다.

### Card 앞면은 Sense의 종류가 정한다

| Sense 종류 | 앞면에 보이는 것 |
|---|---|
| `word` | 그 단어의 표기형 (`run`) |
| `phrase` | 관용구 구문 자체 (`give up`) |
| `example` | 예문 (`She runs every morning.`) |

뒷면은 언제나 뜻풀이(`definition`)다. 앞면을 서버가 정해서 보내므로 화면이 다시
계산하지 않는다.

### 삭제는 물리 삭제가 아니다

Card·Deck·Sense를 지우면 `deleted_at`(또는 `archived_at`)이 찍힌다. 평소 조회에서는
안 보이게 걸러내지만, **동기화 경로에서는 "사라진 사실"만 tombstone으로 전달된다.**
여러 기기가 같은 계정을 쓰려면 이게 있어야 한다.

---

## 리눅스 데스크톱 앱으로 등록하고 실행하기

이 웹 애플리케이션을 리눅스(KDE, GNOME 등 FreeDesktop 표준) 데스크톱 앱으로 등록하여
런처 메뉴 검색과 전용 독립 앱 창 모드로 실행할 수 있습니다.

### 1. 앱 등록 (최초 1회)

```bash
./scripts/install-desktop.sh
```

- 시스템 아이콘 경로(`~/.local/share/icons/hicolor/`)에 벡터 SVG 및 멀티사이즈 PNG(128, 64, 48, 32, 16px) 아이콘이 설치됩니다.
- 애플리케이션 엔트리(`~/.local/share/applications/voca-memory.desktop`)가 생성되어 시스템 앱 메뉴/검색에 즉시 노출됩니다.

### 2. 앱 실행

- **시스템 런처 / 검색**: KRunner, Kickoff, GNOME 애플리케이션 검색에서 `Voca Memory` 검색 후 실행
- **터미널 실행**:
  ```bash
  ./scripts/launch-desktop.sh
  ```

런처는 백그라운드에서 `voca-server`가 동작 중인지 확인하고, 꺼져 있다면 전용 DB(`~/.local/share/voca-memory/voca.db`)와 함께 자동 기동한 뒤 브라우저를 전용 앱 창(`--app=...`)으로 띄웁니다.

### 3. 등록 해제 (언인스톨)

```bash
./scripts/uninstall-desktop.sh
```

---

## 서버 직접 띄우기 (개발자/서버 모드)

먼저 툴체인이 sqlx의 요구를 만족해야 한다. `Cargo.lock`의 sqlx 0.9.0은 **rustc
1.94+** 를 요구하는데 워크스페이스 `rust-version`(`1.88`)보다 높다. 1.88에서는
`cargo build` 가 sqlx 설명과 함께 시작부터 실패한다. 아래가 전부:

```bash
rustc --version            # 1.94 미만이면 먼저 rustup update stable
cargo build
```

개발용 기동 (브라우저로 `http://127.0.0.1:3000` 을 여는 경우):

```bash
VOCA_DB_PATH=data/dev.db \
VOCA_HOST=127.0.0.1 \
VOCA_PORT=3000 \
VOCA_ALLOWED_ORIGINS=http://127.0.0.1:3000 \
VOCA_SECURE_COOKIES=0 \
./target/debug/voca-server
```

`VOCA_ALLOWED_ORIGINS` 는 **열어 둔 주소와 정확히 같아야 한다.** 위 예에서
브라우저 주소가 `http://localhost:3000` 이면 allowlist도 `localhost` 로 줘야 한다.
`data/` 와 `*.db` 는 `.gitignore` 에 있어 로컬 DB가 커밋에 섞이지 않는다.

API 만 curl 로 찍을 거면 주소가 아무 의미가 없으니 `https://voca.example.kr` 처럼
정한 값을 그대로 쓰면 된다.

### 환경 변수

설정 파일은 없고 환경변수만 읽는다 (`crates/voca-server/src/config.rs`).

| 변수 | 필수 | 기본 | 설명 |
|---|:-:|---|---|
| `VOCA_DB_PATH` | **예** | 없음 | SQLite 파일 경로. **기본값이 없다** — 개발용 DB와 실사용 DB가 섞이는 걸 막으려 일부러 없앴다 |
| `VOCA_HOST` | 아니오 | `127.0.0.1` | **기본이 루프백이다.** `0.0.0.0` 이 아니다 |
| `VOCA_PORT` | 아니오 | `3000` | |
| `VOCA_ALLOWED_ORIGINS` | 아니오 | 비어 있음 | 콤마로 구분. **비어 있으면 모든 변경 요청이 거부된다** (아래) |
| `VOCA_SECURE_COOKIES` | 아니오 | `true` | 로컬 HTTP로 개발할 때만 `0` 또는 `false` |
| `VOCA_LOG` | 아니오 | `voca_server=info,tower_http=warn,warn` | tracing 필터 |

### 두 가지 함정

**1. `VOCA_ALLOWED_ORIGINS`가 비면 서비스가 죽는다.** 변경 요청(POST)을 서버가
전부 거부한다. 부팅은 성공하고 서비스는 살아 있지만 아무것도 저장이 안 된다. 서버는
경고를 남기지만 멈추지 않는다:

```
WARN voca_server: VOCA_ALLOWED_ORIGINS 가 비어 있다. 모든 변경 요청이 거부된다.
```

로컬에서 `http://127.0.0.1:3000` 으로 열 거면 값을 정확히 넣어라.

**2. `Secure` 쿠키와 HTTP.** 기본값이 `true` 라서 로컬에서 `http://` 로 열면
브라우저가 세션 쿠키를 버린다 — 로그인 직후 다시 로그인된 것처럼 보인다. 로컬
개발은 `VOCA_SECURE_COOKIES=0` 을 반드시 함께 쓴다.

**`VOCA_HOST`를 `0.0.0.0` 으로 열기 전에 CSRF 목록부터 채워라.** 빈 allowlist와
바깥 공개는 가장 나쁜 조합이다.

---

## 브라우저에서 직접 쓰기

브라우저로 `http://127.0.0.1:3000/` 을 열면 **대시보드**가 보인다. 서버가 처음부터
그린다(SSR) — 로그인한 사람이면 그 사람 수치로, 아니면 0으로.

표시되는 것:

- `N일 연속` — 현재 Streak. **할 게 남아 있는데 오늘 아직 안 했다면** 옆에
  "오늘 아직 안 했습니다" 가 붙는다.
- `N장 복습 · M장 새 단어`
- `Lv N` + 진행 바

### 왜 이렇게 보이나

- 로그인 안 한 사람에게도 401 이 아니라 빈 대시보드를 그린다. 401 JSON 을 페이지에
  띄우는 것보다 낫다.
- Streak 위험 표시는 **"할 게 있는데 안 한 경우"** 다. 할 게 없으면 아무 일도 안
  일어나도 끊긴 게 아니다 — 다시 할 수 있으니까.

### 지금 안 되는 것 (중요)

- **버튼이 하나도 없다.** 복습 4버튼(`Again`/`Hard`/`Good`/`Easy`) 컴포넌트
  (`RatingButtons`)는 `crates/voca-ui/src/view.rs` 에 정의돼 있지만 `App` 트리에
  **마운트돼 있지 않다.** 복습은 지금 API 로만 가능하다.
- **로그인 폼이 없다.** 회원가입·로그인 UI 가 없어서 브라우저로는 세션을 만들 수
  없다. API 로 로그인한 뒤 쿠키가 있으면 대시보드가 채워진다.
- **스타일은 있다.** `crates/voca-ui/assets/retro.css` 하나이고, `render_page` 가
  `include_str!` 로 읽어 `<style>` 태그 안에 넣어 보낸다. 서버에 정적 파일 서빙
  라우트가 없기 때문이다 — 그래서 **CSS 를 고쳐도 서버를 다시 빌드해야** 바뀐다.
  외부 폰트·CDN 은 쓰지 않는다(오프라인에서 조용히 사라진다).
- **wasm 번들이 404 난다.** `/pkg/voca-web.js` 를 참조하지만 `cargo-leptos` 도
  `web/` 디렉터리도 없다. 그래서 하이드레이션이 일어나지 않는다. 화면은 **보이지만
  아무것도 반응하지 않는다** — 지금은 원래 반응할 게 없으니 체감은 없다.
  wasm 을 읽지 못하면 값을 지어내지 않고 하이드레이션을 아예 건너뛴다.

---

## API 로 쓰기

브라우저 UI 가 없는 만큼, 지금 이 앱을 쓰는 방법은 API 다. 아래는 전부 실제로
동작하는 순서로 적어둔 것이다.

### 준비

```bash
BASE=http://127.0.0.1:3000
ORIGIN=https://voca.example.kr
JAR=/tmp/voca-cookies.txt          # 세션 쿠키가 여기 저장된다
```

**모든 변경 요청에 `Origin` 헤더가 반드시 필요하다.** 안 보내면 거부된다(400).
브라우저는
자동으로 보내지만 curl 은 직접 줘야 한다. `Referer` 로도 대신할 수 있다.

### 1. 회원가입

```bash
curl -s -X POST "$BASE/api/auth/register" \
  -H "Origin: $ORIGIN" -H 'Content-Type: application/json' \
  -c "$JAR" \
  -d '{"email":"me@b.kr","password":"부지런한-비밀번호","display_name":"홍길동","timezone":"Asia/Seoul","retention":null}'
```

```json
{"user_id":"019...","display_name":"홍길동","retention":"balanced"}
```

- `timezone` 은 **필수** (IANA 이름). 비우면 400. 다만 **표시 전용** — 스트릭 판정은
  서버의 로컬 시간대를 쓴다. 클라이언트가 조작해 봤자 영향이 없다.
- `retention` 은 `null` 이면 기본값 `balanced` 가 들어간다. 값을 주려면
  `"diligent"` / `"balanced"` / `"frugal"` 중 하나. (이 필드만 생략해도 된다.)
- **세션 토큰은 본문에 없다.** `Set-Cookie` 로만 나간다. (JSON 에 실으면 XSS 스크립트가
  읽을 수 있다.) 그래서 `-c "$JAR"` 로 쿠키를 받아 둬야 한다.
- 비밀번호는 **8자 이상, 256바이트 이하.** 글자 수가 아니라 **문자 수**라서
  한글 8자면 된다.
- 이메일 형식이 엄격하다. `@` 가 정확히 하나, 로컬 부분이 비면 안 되고, 도메인에
  점이 있어야 하며, 점이 앞뒤를 두드리거나 연속하면 안 된다.

### 2. 내 뜻 추가

```bash
curl -s -X POST "$BASE/api/senses" \
  -H "Origin: $ORIGIN" -H 'Content-Type: application/json' -b "$JAR" \
  -d '{"lemma":"run","kind":"word","definition":"달리다","pos":"verb","example_en":null}'
```

```json
{"sense_id":"019...","word_id":"019...","kind":"word","definition":"달리다","example_en":null,"pos":"verb"}
```

- `kind` 는 `word` / `phrase` / `example` 중 하나.
- `definition` 은 **비어 있으면 안 된다.**
- 같은 표기형이 이미 있으면 그 Word 아래에 붙고, 없으면 Word 도 함께 만들어진다.
- 여기서 만든 Sense 는 `source = user` 로 저장된다. 사전에서 온 것과 구분된다.

### 3. 덱 만들기

```bash
curl -s -X POST "$BASE/api/decks" \
  -H "Origin: $ORIGIN" -H 'Content-Type: application/json' -b "$JAR" \
  -d '{"name":"토익 3000"}'
```

```json
{"deck_id":"019...","name":"토익 3000","daily_goal":20,"new_per_day":10}
```

`daily_goal`(하루 목표 복습 수, 기본 20)과 `new_per_day`(하루 신규 수, 기본 10)은
**서로 독립**이다. 생략하면 위 기본값이 들어간다.

### 4. 덱에 Card 로 추가

```bash
curl -s -X POST "$BASE/api/decks/cards" \
  -H "Origin: $ORIGIN" -H 'Content-Type: application/json' -b "$JAR" \
  -d "{\"deck_id\":\"$DECK_ID\",\"sense_ids\":[\"$SENSE_ID\"]}"
```

한 번에 최대 200장. **같은 Sense 를 두 번 넣어도 한 장만 생긴다** — 이미 있는
카드는 건너뛴다.

### 5. 사전 조회

```bash
curl -s "$BASE/api/dict/lookup?lemma=abandon" -H "Origin: $ORIGIN" -b "$JAR"
```

```json
{"status":"fetched","word":{"word_id":"019...","lemma":"abandon","phonetic":"/əˈbændən/",
 "senses":[{"kind":"word","pos":"verb","definition":"...","example_en":"..."}]}}
```

외부 `dictionaryapi.dev` 에서 가져와 로컬 DB 에 캐시한다. 두 번째 조회는 캐시에서
나온다. `status` 값으로 상황을 구별해 준다 — **전부 200 이지만 뜻이 다르다.**

| `status` | 뜻 | 할 일 |
|---|---|---|
| `fetched` | 외부에서 가져와 저장까지 성공 | "덱에 추가" 가능 |
| `cached` | 이미 캐시에 had다 | "덱에 추가" 가능 |
| `unstored` | 뜻은 봤는데 **저장 안 됐다** | `word_id` 가 `null`. 덱에 추가 불가 |
| `not_found` | 사전에 그 단어 없음 | 오타일 수 있음 |
| `unavailable` | 외부 사전이 죽었거나 느림 | 나중에 다시 시도 |
| `unrecognized` | 응답 형식을 못 알아봤다 | 서버 문제 |

**`unstored` 를 조용히 성공으로 취급하지 않는 게 요점이다.** 뜻은 화면에 보이는데
"덱에 추가" 를 눌러도 아무 일도 안 일어나는 상태를 사용자에게 숨기지 않는다.
`unstored` 면 `word_id` 가 `null` 이므로 덱에 넣을 수 없다 — 직접 `/api/senses`
로 만들어 넣어야 한다.

로그인이 있어야 조회된다. 익명으로 상위 어휘를 긁어내는 걸 막기 위해서다.
외부 요청 실패는 10분간 캐시된다 — 매번 다시 시도하지 않는다.

### 6. 복습 큐

```bash
curl -s "$BASE/api/study/queue" -H "Origin: $ORIGIN" -b "$JAR"
```

```json
{"cards":[{"card_id":"019...","deck_id":"019...","lemma":"run","phonetic":"/ɹʌn/",
  "front":{"kind":"lemma","text":"run"},
  "definition":"달리다","pos":"verb","example_ko":null,"is_new":true}],
 "reviews_remaining":1,"new_remaining_today":0}
```

- 기본 20장, 최대 100장. `?limit=50` 으로 바꾼다.
- `?deck=<deck_id>` 로 덱을 좁힌다. **깨진 id 면 400** 이고, 조용히 "전체 덱" 으로
  바뀌지 않는다.
- 정렬은 **회수 가능성이 낮은 순** — 곧 잊을 것부터 보여준다. 신규는 `new_per_day`
  만큼만 섞인다.
- `front.kind` 가 `lemma` 면 `front.text` 가 표기형, `example` 면 예문이다.
- `is_new` 이 `true` 면 아직 한 번도 안 본 Card.

### 7. 4버튼 평가

```bash
curl -s -X POST "$BASE/api/study/review" \
  -H "Origin: $ORIGIN" -H 'Content-Type: application/json' -b "$JAR" \
  -d "{\"card_id\":\"$CARD_ID\",\"rating\":\"good\",\"duration_ms\":4200}"
```

`rating` 은 `again` / `hard` / `good` / `easy` 중 하나. `duration_ms` 는 선택.

```json
{"card_id":"019...","due_at":"2026-09-30T09:00:00+09:00","scheduled_days":3.0,
 "preview":{"again":{"label":"지금","days":0},"hard":{"label":"2일 후","days":2},
            "good":{"label":"3일 후","days":3},"easy":{"label":"9일 후","days":9}},
 "streak":{"current":7,"longest":12,"reviewed_today":true},
 "xp_earned":2,"level":{"level":3,"xp_into_level":300,"xp_span":1200},
 "requeue":false}
```

- **`preview` 가 다음 카드 버튼 라벨이다.** 서버가 계산해서 준다. 화면이 다시
  계산하지 않는다 — 그러면 라벨과 실제 예약 시각이 어긋난다.
- `requeue` 이 `true` 면 **같은 세션에서 이 카드가 다시 나온다** (`scheduled_days < 1`
  일 때). 새로 배운 단어를 잊지 않게 하기 위한 것이다.
- 한 번의 요청이 4개 테이블을 한 트랜잭션으로 갱신한다. 중간에 실패하면 전부
  롤백된다.
- 깨진 `card_id` 는 400. 조용히 무시되지 않는다.

### 8. 대시보드

```bash
curl -s "$BASE/api/dashboard" -H "Origin: $ORIGIN" -b "$JAR"
```

```json
{"streak":{"current":7,"longest":12,"reviewed_today":true},
 "level":{"level":3,"xp_into_level":300,"xp_span":1200},
 "reviews_due":1,"new_remaining":0,
 "decks":[{"deck_id":"019...","total":50,"seen":30,"due":1,"fresh":20,"progress_percent":60}],
 "streak_at_risk":false}
```

`streak_at_risk` 는 **할 게 남았는데 오늘 아직 안 한 경우**다. 할 게 없으면
`false` 다 — 아무것도 안 해도 다시 할 수 있으니까 끊긴 게 아니다.

### 9. 동기화

```bash
curl -s "$BASE/api/sync/changes?since=0" -H "Origin: $ORIGIN" -b "$JAR"
```

```json
{"changes":[{"kind":"card","item":{...},"revision":3}],
 "watermark":12,"has_more":false}
```

- `since` revision 이후 바뀐 행만 준다. 한 번에 최대 1000건, 넘으면 `has_more` 가
  `true` 가 되고 **같은 `since` 로 다시 부른다.**
- 다 적용한 뒤 `watermark` 를 저장해 둔다. **변경이 없어도 watermark 는 갱신된다** —
  "확인했다"는 사실도 갱신이다.
- soft delete 된 행은 `card_tombstone` / `deck_tombstone` / `sense_tombstone` 으로
  온다. 지워진 카드의 정의는 실려 오지 않는다.
- `card` 항목에는 `memory` (스케줄러 상태) 가 들어있다. 이게 여러 기기 사이에
  외운 상태를 옮기는 통로다.

### 10. 로그아웃

```bash
curl -s -X POST "$BASE/api/auth/logout" -H "Origin: $ORIGIN" -b "$JAR" -c "$JAR"
```

204 를 돌려주고 쿠키를 만료시킨다. 세션도 서버에서 지운다.

### 나머지

- `GET /api/auth/me` — 현재 사용자. **토큰이 없으면 401 이 아니라 `null`** 을 준다.
  대시보드가 "로그인 안 됨"을 조용히 알아야 하기 때문이다.
- `GET /api/health` — `"ok"`. 인증 불필요.

---

## 게임화 규칙

### XP

Rating 에 단순 매핑. 난이도 보정도, 재성공 보너스도 없다.

| Rating | XP | 이유 |
|---|:-:|---|
| `Again` | **0** | 실패에 보상을 주면 `Again` 을 안 누르게 되고, FSRS 가 필요한 신호가 오염된다. 보상할 대상은 복습 행위이지 정답이 아니다 |
| `Hard` | 1 | |
| `Good` | 2 | |
| `Easy` | 4 | |

### Level

```
level = floor(sqrt(total_xp / 200)) + 1
```

기초 구간이 빠르고 시간이 갈수록 평평해진다. 상한은 없다.

### Streak

- 하루에 Review 가 1건 이상이면 그날이 이어진다. **Rating 이 무엇이든** —
  `Again` 만 눌러도 그날은 Streak 에 포함된다.
- **날짜 경계는 접속 시점으로 고정된다.** 브라우저를 자정 넘겨 열어놔도 그날
  처음 접속한 시각 기준으로만 판정한다. 브라우저를 켜놓고 밤을 넘기는 것으로는
  Streak 가 안 뒤집힌다.
- `users.timezone` 은 **표시 전용**이다. Streak 판정에 쓰지 않는다. 여행 중 접속하면
  timezone 이 바뀌어 이미 기록된 날짜가 달라져 버리기 때문이다.
- 세션 도중 연결이 끊겼다가 다음 날 다시 붙으면 새 세션이라 경계가 바뀐다. 이
  경우는 아직 규칙이 없다.

###.Retention Preset

숫자가 아니라 세 칸만 노출한다. 사용자가 `0.90` 의 의미를 알 리 없다.

| 프리셋 | `desired_retention` | 일일 리뷰 | 성숙 단어 | 특징 |
|---|:-:|---:|---:|---|
| `부지런` (Diligent) | 0.95 | 17.0건 | 98% | 가장 자주, 가장 오래 |
| `균형` (Balanced) | 0.90 | 10.5건 | 95% | 기준값 (기본) |
| `아끼기` (Frugal) | 0.85 | 7.3건 | 92% | 적게, 까먹을 위험 최대 |

위 숫자는 덱 500단어 기준 실측이다. **부하는 덱 크기에 거의 선형으로 증가한다.**
덱이 2000단어면 `부지런` 기준으로 61건/일, 7.7분이 된다. 프리셋을 바꾸면 일일
복습량이 크게 달라진다는 걸 알고 골라야 한다.

---

## 자주 놀라운 동작

**성숙한 단어를 잊었는데 카드가 사라진다.** `Again` 을 눌러도 성숙한 단어는
오늘 다시 나오지 않고 이틀 뒤에 나온다. 버그처럼 보이지만 알고리즘이 정한 정상
동작이다. 이 앱은 learning step 없이 순수 FSRS-6 만 쓴다.

| 상태 | `Again` 을 누르면 |
|---|---|
| 새 단어 | 0.21일(약 5분) → 오늘 다시 나옴 |
| 성숙한 단어 (Stability 30일) | 2.3일 → 오늘은 안 나옴 |

화면은 "이 단어는 내일 다시 만납니다" 라고 안내만 한다. 스케줄은 FSRS 가 정한 대로
둔다. `preview.again.label` 이 "지금" 이 아니라 "2일 후" 로 오는 것으로 확인할 수
있다.

**"또 모름" 이 XP 0이다.** 위 표와 같은 이유다. 의도된 설계다.

**`card_states` 의 두 상태.** `new` 와 `review` 뿐이다. Anki 의
`new | learning | review | relearning` 네 단계가 아니다. 순수 FSRS-6 은 learning step
을 모델링하지 않는다. 재방문은 짧은 interval 로 만든다.

**Card 앞면이 뜻이 아니라 예문일 수 있다.** `kind: example` 로 넣은 Sense 는 앞면에
예문이 나온다. 뜻이 여러 개인 단어를 문맥째로 외우게 하려는 것이다.

---

## 오류 응답

모든 오류는 같은 모양이다. **기계용 `code` 와 사람용 `message` 가 분리**돼 있다.

```json
{"code":"bad_credentials","message":"..."}
```

| `code` | 상태 | 뜻 |
|---|:-:|---|
| `bad_credentials` | 401 | 로그인 안 됨 / 세션 만료. **계정 없음과 비밀번호 오류를 구분하지 않는다** — 구분해 주면 누가 이메일 존재 여부를 알 수 있다 |
| `email_taken` | 409 | 그 이메일이 이미 있다 |
| `rejected` | 400 | 입력이 규칙을 어겼다. **또는 출처가 신뢰되지 않는다** — CSRF 거절도 400 으로 내려온다 |
| `too_many_attempts` | 429 | 로그인 시도 초과. `Retry-After: 900` |
| `server_down` | 503 | 저장소가 죽었다. **비밀번호 오류가 아니라 서버 문제** — 재시도하면 된다 |

**서버가 죽었는데 "비밀번호가 틀렸습니다" 를 주지 않는다.** 사용자가 비밀번호를
계속 바꾸게 만들기 때문이다. 원인이 다르면 대응도 다르므로 상태 코드로 구분한다.

### 로그인 제한

15분 안에 8회 실패하면 `(이메일, IP)` 쌍으로 잠긴다. 둘을 같이 세는 이유는 한쪽만
잠그면 다른 쪽으로 우회할 수 있기 때문이다. 로그인에 성공하면 즉시 초기화된다.
한 계정을 잠가도 다른 계정은 막히지 않는다.

잠금 상태라도 `503` 과 `401` 은 구분된다 — 서버가 죽었을 때 그걸 "비밀번호 오류" 로
알려주면 안 되기 때문이다.

---

## CSRF

변경 요청(POST 등)은 출처를 검사한다. 신뢰하는 출처는 `VOCA_ALLOWED_ORIGINS` 다.

```bash
# 신뢰된 출처 → 통과
curl -X POST "$BASE/api/study/review" -H 'Origin: https://voca.example.kr' ...   # 200

# 다른 사이트 → 거부
curl -X POST "$BASE/api/study/review" -H 'Origin: https://evil.example.com' ...  # 400 rejected
```

**조회(GET)는 검사하지 않는다.** 읽기는 부작용이 없다.

세션 쿠키는 `HttpOnly` + `SameSite=Lax` 다. XSS 스크립트가 쿠키를 읽을 수 없고,
다른 사이트가 만든 form POST 로는 쿠키가 안 나간다.

`Origin` 이 아예 없는 요청(테스트, 데스크톱 앱, curl)은 **기본적으로 허용**된다.
기본값이 `true` 인 이유는 그 경로가 전부 막히면 안 되기 때문이다. 웹 브라우저
경로에서만 막아야 하고, 그때는 서버 로그에 남긴다.

---

## 실전 흐름 전체

`./scripts/smoke.sh` 가 이 순서를 그대로 12단계로 돌린다. 미리 빌드된
`target/debug/voca-server` 가 필요하다.

```bash
cargo build && ./scripts/smoke.sh
```

1. 회원가입
2. 덱 만들기
3. 내 뜻 추가
4. 덱에 Card 로 추가
5. 복습 큐 받기
6. `Good` 로 평가
7. 대시보드 확인
8. 로그인한 SSR 페이지 확인
9. 다시 평가하면 큐가 비었는지
10. 사전 조회 (네트워크 없이)
11. 동기화 타임라인
12. CSRF — 외부 Origin 으로 변경 요청 → 거부

수동으로 처음부터 따라 하려면:

```bash
export BASE=http://127.0.0.1:3000 ORIGIN=https://voca.example.kr JAR=/tmp/vc.txt
cargo build
VOCA_DB_PATH=/tmp/v.db VOCA_ALLOWED_ORIGINS="$ORIGIN" VOCA_SECURE_COOKIES=0 \
  ./target/debug/voca-server &

# 1~7단계
# 회원가입 → 덱 → 뜻 → Card → 큐 → 평가 → 대시보드 순으로 위의 curl 을 차례로 실행
```

---

## 데이터는 어디에 있나

전역 어휘 사전(`words`/`senses`)과 사용자 소유 학습 재료(`decks`/`cards`/
`card_states`)가 **분리**돼 있다. 단어는 공유하고 복습 기록은 공유하지 않는다.

```
users
├─ words · senses          전역 사전 + 사용자 정의 Sense (source = user)
├─ decks
│  └─ cards
│     └─ card_states       스케줄러 상태 (Stability · Difficulty · due_at)
├─ review_log              append-only. Review 의 원장
├─ streaks                 review_log 에서 파생한 캐시
└─ xp_events               append-only. Review 에서 파생
```

- `streaks` 와 `xp_events` 는 **파생 데이터**다. `review_log` 가 진짜 원장이다.
- 로컬 DB 파일(`*.db`, `*.db-wal`)은 `.gitignore` 에 있다 — 평문 비밀번호 해시와
  세션 토큰이 들어있으므로 절대 커밋하지 않는다.
- 배포는 **단일 VPS 의 SQLite 파일**이다 (WAL 모드). 동시 쓰기는 낙관적 동시성
  (`WHERE rev = ?`)으로 처리하고, 충돌하면 트랜잭션을 되감아 재시도한다.

---

## 용어집

`CONTEXT.md` 가 전체 용어집이다. 여기서 자주 헷갈리는 것만.

| 용어 | 뜻 | 자주 혼동되는 것 |
|---|---|---|
| **Word** | 표기형 하나 | 뜻(Sense)과 다르다 |
| **Sense** | 뜻 하나 | 뜻풀이 문장(definition)과 다르다 |
| **Card** | Sense 하나를 향하는 복습 단위 | Note, Flashcard |
| **Review** | Card 를 보고 Rating 을 매기는 한 번 | Attempt, Answer |
| **Rating** | `Again`/`Hard`/`Good`/`Easy` | Grade, Score |
| **Stability** | 기억이 유지될 기간(일) | Retention, Interval |
| **Due** | `now >= due_at` 인 상태 (**계산 결과**) | Scheduled(저장된 예약 시각) |
| **Mastery** | Word/Card 숙련도(파생) | Level(계정 등급) |
| **Revision** | 동기화용 단조 카운터 | Version, ETag |
| **XP / Level** | 계정 단위 진행/등급 | Card 난이도와 무관 |

**Card 의 복제본은 이력을 물려받지 않는다.** `New` 상태로 시작한다. 한 덱에서 외운
것이 다른 덱에 자동 이월되지 않는다 — Anki 와 같은 선택이고, 이월이 필요하면
"Deck 간 전이" 기능을 명시적으로 추가해야 한다.

---

## 안 한 일

- **발음 채점** — 초기 범위 밖. 별도 서브시스템.
- **단어 목록 오프라인 제공** — 전체 카탈로그는 런타임 fetch. 앱 바이너리에 넣지
  않는다. 시드 단어 몇 개만 동봉하고 나머지는 네트워크에 의존한다.
- **네이티브 UI 프레임워크** — Dioxus/egui/Iced 로 바꾸지 않는다.
- **브라우저 복습 화면** — 위 [요약](#요약-한-눈에-보기) 참조. 지금은 API 가 유일한
  경로다.

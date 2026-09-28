# Design

Voca Memory의 설계 문서. 용어는 [`CONTEXT.md`](../CONTEXT.md), 개별 결정의 근거는 [`docs/adr/`](./adr/)에 있다.

## 목표

- 여러 의미(Sense)에 걸친 영어 어휘를 개별적으로 복습하고, 그 기억 상태를 계량한다.
- 복습 스케줄은 FSRS-6으로 계산한다. [`fsrs`](https://crates.io/crates/fsrs) 크레이트를 직접 쓰고, 재작업하지 않는다.
- 개인 프로젝트로 공개 배포한다. 남이 설치해서 쓸 수 있어야 하지만, 직접 운영하지는 않는다.
- 웹을 첫 타겟으로 배포하고, 같은 UI를 데스크톱·모바일로 확장한다.

## 비목표

- **Pronunciation Score** — 발음 채점은 초기 범위 밖. 별도 서브시스템으로 다룬다.
- **실시간 협업 편집** — 동기화는 서버 중재 last-write-wins로 충분하다 ([ADR-0005](./adr/0005-revision-column-now-sync-later.md)).
- **네이티브 UI 프레임워크** — Dioxus/egui/Iced로 갈아타지 않는다 ([ADR-0001](./adr/0001-leptos-over-dioxus-for-the-web-app.md)).
- **전체 단어 목록 오프라인 제공** — 시드 소량만 동봉하고 나머지는 네트워크에 의존한다 ([ADR-0007](./adr/0007-word-data-is-fetched-at-runtime-not-bundled.md)).

## 아키텍처

### 크레이트 구성

```
crates/
  voca-domain/          FSRS 스케줄러, 게임화 규칙, 통계 집계 — 순수 Rust
  voca-store/           저장소 trait (도메인만 의존)
  voca-store-sqlite/    sqlx 기반 어댑터
  voca-dict/            외부 사전 API 클라이언트 + 캐시
  voca-ui/              Leptos 컴포넌트 (voca-domain만 참조)
  voca-server/          인증 규칙 + argon2 + axum 라우터
web/                    SSR entry, CSR entry, index.html
migrations/             voca-store-sqlite/migrations 에 있다
```

`voca-store-sqlite` 안에는 어댑터가 세 개 있다. `Store`(학습 자료), `Accounts`(계정),
`Sessions`(로그인) — 하나씩 다른 이유를 갖기 때문에 따로 둔다.

`src-tauri/`는 Phase 5에서 추가한다. 그 시점에도 `voca-domain`과 `voca-ui`를 그대로 재사용한다.

### 의존 규칙

의존은 아래 방향으로만 흐른다. 역방향은 어떤 경우에도 금지.

| 크레이트 | 의존할 수 있는 것 |
|---|---|
| `voca-domain` | `serde`, `time`, `uuid`, `fsrs` — 그 외 없음 |
| `voca-store` | `voca-domain` |
| `voca-store-sqlite` | `voca-store`, `sqlx` |
| `voca-dict` | `voca-domain`, `reqwest` |
| `voca-ui` | `voca-domain`, `leptos` |
| `voca-server` | 위 전부 + `axum`, `argon2`, `uuid` |
| `voca-sim` | `voca-domain`, `fsrs` — 네이티브 전용 개발 도구 |

`getrandom`은 `voca-domain`의 wasm 빌드에만, `cfg(target_arch = "wasm32")`로 걸린다. `fsrs`가 `rand`를 통해 요구하는데 브라우저 스케줄러는 난수를 쓰지 않는다 ([ADR-0008](./adr/0008-getrandom-wasm-shim.md)).

`voca-domain`이 `serde_json`을 쓰지만 그것은 **dev-dependency**다. Wasm 번들에 들어가지 않으므로 규칙 대상이 아니다.

`./scripts/check-domain-boundaries.sh`가 세 가지를 검사하고, 하나라도 깨지면 exit 1이다.

1. `voca-domain`의 직접 의존성이 허용 목록 안에 있는가 (`--edges normal`, dev-dep 제외)
2. `voca-domain`이 `wasm32-unknown-unknown`에서 실제로 컴파일되는가
3. 공개 API에 `async` 함수가 새어들지 않았는가

2번이 없으면 `fsrs`의 전이 의존이 슬그슬 바뀌어도 브라우저에서 깨지는 걸 CI가 잡지 못한다. 실제로 `getrandom`이 이 방식을 막은 바 있다.

## 데이터 모델

전역 어휘 사전(`words`/`senses`)과 사용자 소유 학습 재료(`decks`/`cards`/`card_states`)를 분리한다. 단어는 공유하고 복습 기록은 공유하지 않는다. 다만 사용자 정의 Sense는 같은 테이블에 `source = user`로 들어간다 ([ADR-0006](./adr/0006-sense-covers-more-than-dictionary-definitions.md)).

```sql
-- 계정
users(
  id            TEXT PRIMARY KEY,   -- uuid v7
  email         TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,      -- argon2id
  display_name  TEXT NOT NULL,
  timezone      TEXT NOT NULL,      -- IANA. 표시용. Streak 판정에는 쓰지 않는다
  retention     TEXT NOT NULL,      -- Retention Preset: diligent | balanced | frugal
  created_at    INTEGER NOT NULL
);

-- 어휘 사전. source = dictionary 행은 읽기 전용(임포트로만 생성)
words(
  id         TEXT PRIMARY KEY,
  lemma      TEXT NOT NULL,
  source     TEXT NOT NULL,         -- dictionary | user
  phonetic   TEXT,
  audio_url  TEXT,
  rev        INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX words_dict_lemma ON words(lemma) WHERE source = 'dictionary';
CREATE UNIQUE INDEX words_user_lemma ON words(lemma, source) WHERE source = 'user';

senses(
  id          TEXT PRIMARY KEY,
  word_id     TEXT NOT NULL REFERENCES words(id),  -- kind=example여도 NOT NULL
  kind        TEXT NOT NULL,        -- word | phrase | example
  source      TEXT NOT NULL,        -- dictionary | user
  pos         TEXT,
  definition  TEXT NOT NULL,        -- 비어 있으면 안 된다 (사용자 Sense)
  example_en  TEXT,
  example_ko  TEXT,
  archived_at INTEGER,              -- 품질 게이트. 쓰레기 Sense를 숨긴다
  rev         INTEGER NOT NULL DEFAULT 0,
  updated_at  INTEGER NOT NULL
);

-- 사용자 소유 학습 재료
decks(
  id          TEXT PRIMARY KEY,
  user_id     TEXT NOT NULL REFERENCES users(id),
  name        TEXT NOT NULL,
  description TEXT,
  daily_goal  INTEGER NOT NULL DEFAULT 20,   -- 하루 목표 복습 건수
  new_per_day INTEGER NOT NULL DEFAULT 10,   -- 하루 신규 Card 수. daily_goal과 독립
  rev         INTEGER NOT NULL DEFAULT 0,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
);

cards(
  id          TEXT PRIMARY KEY,
  user_id     TEXT NOT NULL REFERENCES users(id),
  deck_id     TEXT NOT NULL REFERENCES decks(id),   -- 정확히 하나
  sense_id    TEXT NOT NULL REFERENCES senses(id),  -- 정확히 하나
  cloned_from TEXT REFERENCES cards(id),            -- 복제 원본
  rev         INTEGER NOT NULL DEFAULT 0,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
);

card_states(
  card_id        TEXT PRIMARY KEY REFERENCES cards(id),
  state          TEXT NOT NULL,      -- new | review  (4단계가 아님, 아래 설명)
  stability      REAL,               -- 일 단위
  difficulty     REAL,               -- 1.0 ~ 10.0
  elapsed_days   REAL,
  scheduled_days REAL,
  due_at         INTEGER NOT NULL,  -- unix 초
  last_review_at INTEGER,
  reps           INTEGER NOT NULL DEFAULT 0,
  lapses         INTEGER NOT NULL DEFAULT 0,
  rev            INTEGER NOT NULL DEFAULT 0,
  updated_at     INTEGER NOT NULL
);

review_log(
  id             TEXT PRIMARY KEY,
  user_id        TEXT NOT NULL,
  card_id        TEXT NOT NULL,
  rating         INTEGER NOT NULL,  -- 1=Again 2=Hard 3=Good 4=Easy
  reviewed_at    INTEGER NOT NULL,
  local_date     TEXT NOT NULL,     -- YYYY-MM-DD. Streak 판정용
  duration_ms    INTEGER,
  prev_stability REAL,
  next_stability REAL,
  rev            INTEGER NOT NULL DEFAULT 0,
  updated_at     INTEGER NOT NULL
);

-- review_log에서 파생한 캐시. Review 1건이 추가되면 갱신한다.
streaks(
  user_id          TEXT PRIMARY KEY,
  current_count    INTEGER NOT NULL DEFAULT 0,
  longest_count    INTEGER NOT NULL DEFAULT 0,
  last_review_date TEXT,            -- review_log.local_date와 같은 형식
  rev              INTEGER NOT NULL DEFAULT 0,
  updated_at       INTEGER NOT NULL
);

-- append-only. review_log에서 파생하므로 rev 불필요
xp_events(
  id          TEXT PRIMARY KEY,
  user_id     TEXT NOT NULL,
  kind        TEXT NOT NULL,        -- review | streak_bonus | daily_goal
  amount      INTEGER NOT NULL,
  source_id   TEXT,
  occurred_at INTEGER NOT NULL
);
```

### Card의 앞면은 kind가 정한다

`senses.word_id`는 항상 NOT NULL이다. `kind = example`도 예외가 아니다 — 그 Sense는 설명되는 **원래 단어**에 속하고, Card 앞면으로 `example_en`을 보여준다. `kind = phrase`만 구문 자체가 새 Word가 된다.

전체 표와 근거는 [ADR-0011](./adr/0011-card-front-is-determined-by-sense-kind.md).

## 스케줄링

`fsrs::FSRS::next_states(previous_state, desired_retention, days_elapsed)`가 4개 Rating의 결과를 한 번에 돌려준다. 이 결과를 Card의 Memory State에 기록한다.

`voca-domain`의 `Scheduler`는 이 호출만 감싼다. 결정적이며 rayon과 난수를 쓰지 않는다 — 어디서 실행하든 같은 결과가 나온다.

### Review State는 두 값뿐이다

`card_states.state`는 `new | review` 두 값이다. Anki의 `new | learning | review | relearning` 네 단계를 따르지 않는다. 순수 FSRS-6은 learning step을 모델링하지 않기 때문이다 ([ADR-0009](./adr/0009-no-learning-steps-pure-fsrs-6.md)).

### 세션 내 재제시

`scheduled_days < 1`이면 그 Card는 오늘 안에서 다시 보여줘야 한다. 순수 FSRS-6은 learning step 대신 짧은 interval로 이 동작을 만든다. 서버는 이 조건으로 세션 내 큐를 만든다. 최소 간격은 1분이다.

**성숙도에 따라 `Again`의 의미가 갈린다 — 사용자에게 설명해야 한다.**

| Card 상태 | `Again`을 눌렀을 때 |
|---|---|
| 새 단어 | 0.21일 (약 5분) → 오늘 다시 나옴 |
| 성숙한 단어 (Stability 30일) | 2.3일 → 오늘은 안 나옴, 이틀 뒤 |

"또 모름"을 눌렀는데 카드가 사라지는 것이 버그처럼 보일 수 있다.

**결정: 가만히 두고 안내만 한다.** 스케줄을 FSRS가 그대로 결정하고, study 화면이 "이 단어는 내일 다시 만납니다"를 보여준다. 알고리즘을 건드리지 않는다.

되돌리려면 learning step을 도입해야 하는데, 그건 XP의 1:1 대응을 깨뜨린다. **이 결정을 되돌리려면 UI가 아니라 알고리즘을 바꿔야 한다.** 사용자 반응이 실제로 불만으로 나타나면 그때 되돌리고, 그전에 미리 우회책을 넣지 않는다.

### Retention Preset

`desired_retention`은 연속값이 아니라 세 개의 프리셋으로만 노출한다. 사용자가 `0.90`이라는 숫자의 의미를 알 리 없기 때문이다.

Phase 1 시뮬레이터(`cargo run -p voca-sim`)가 측정한 값이다. 덱 500단어, 하루 새 단어 10개, learning step 없음, 안정 구간 평균:

| 프리셋 | `desired_retention` | 일일 리뷰 | 최대 리뷰 | 일평균 소요 | 성숙 단어 |
|---|---|---|---|---|---|
| `부지런` (Diligent) | 0.95 | 17.0건 | 73건 | 204초 (3.4분) | 98% |
| `균형` (Balanced) | 0.90 | 10.5건 | 49건 | 126초 (2.1분) | 95% |
| `아끼기` (Frugal) | 0.85 | 7.3건 | 38건 | 88초 (1.5분) | 92% |

부하는 덱 크기에 거의 선형으로 증가한다. 덱 2000단어면 `부지런` 기준 61건/일, 7.7분이다. **프리셋을 고르는 화면에 이 숫자가 직접 보여야 한다** — 0.95를 0.85로 바꾸면 일일 리뷰가 절반이 된다는 사실을 사용자가 알아야 한다.

`decks.daily_goal`의 기본값은 **20**으로 둔다. `균형` 기준 500단어 덱의 평균 부하(10.5건)를 덮으면서, 학습 초기의 작은 덱에도 달성 가능한 수다.

## 게임화 규칙

### XP

Rating에 단순 매핑만 한다. 난이도 보정이나 재성공 보너스를 넣지 않는다.

| Rating | XP |
|---|---|
| `Again` | 0 |
| `Hard` | 1 |
| `Good` | 2 |
| `Easy` | 4 |

`Again`이 0인 것은 의도적이다. 실패에 보상을 주면 학습자가 `Again`을 누르는 걸 꺼리게 되고, 그러면 FSRS가 필요한 신호 자체가 오염된다. 보상이 필요한 것은 복습 행위이지 정답이 아니다.

**이 산식은 고정한다.** `xp_events`가 쌓인 뒤에 바꾸면 과거 데이터 해석이 갈린다.

### Level

```
level = floor(sqrt(total_xp / 200)) + 1
```

**레벨에는 제목 이름을 붙인다.** `7 = 단어 수집가`, `30 = 어휘 대가` 같은 식이다. 코드는 몇 줄이면 끝나고 숫자에 사람이 생긴다. 추가로 마일스톤(단어 100개, 스트릭 100일 등) 달성 시 축하 표시를 준다 — "나도 이만큼 했다"는 순간이 생긴다.

레벨을 기능 잠금에 연결하지는 않는다. 무엇을 잠글지가 억지될 위험이 크고, 보상을 주는 것보다 안 주는 게 쉬운 경우가 많다.

기초 구간은 빠르고, 시간이 갈수록 평평해진다. 상한은 두지 않는다 — 비교 가능성을 잃지 않으면서 오래 플레이할 수 있다.

### Streak

- 하루에 Review가 1건 이상이면 그날이 이어진다. Rating과 무관 — `Again`만 눌러도 그날은 Streak에 포함된다.
- **날짜 경계는 접속 시점으로 고정한다.** 세션 동안 클라이언트가 보낸 UTC offset을 사용하고, 그 세션이 끝날 때까지 고정한다. 브라우저를 자정 넘겨 열어놓아도 Streak이 뒤집히지 않는다.
- 클라이언트가 보내는 offset이 ±14시간을 벗어나면 거부하고 UTC로 떨어뜨린다.
- `users.timezone`은 **표시 전용**이다. Streak 판정에 쓰지 않는다. 여행 중 접속하면timezone이 바뀌어 이미 기록된 날짜가 달라져 버리기 때문이다.
- 판정 결과는 `review_log.local_date`에 기록한다. Streak은 `review_log`에서 파생해 `streaks`에 캐시한다.

**고정하지 않은 채로 남긴다:** 세션 중 접속이 끊겼다가 다음 날 다시 붙으면 새 세션이 시작되므로 경계가 바뀐다. 이 경우의 처리 규칙은 Phase 3에서 실제 UX를 보고 정한다.

## 콘텐츠 파이프라인

```
외부 API ──fetch──▶ 파싱 ──▶ words/senses 캐시
시드 파일 ──────────────────▶ words/senses (라이선스 안전)
사용자 입력 ─────────────────▶ source = user
```

- 전체 카탈로그는 런타임 fetch. 앱 바이너리에는 넣지 않는다 ([ADR-0007](./adr/0007-word-data-is-fetched-at-runtime-not-bundled.md)).
- **시드 단어 30~50개**: 영어 빈도 상위이면서 **여러 뜻과 관용구를 가진 단어**를 우선한다. `use`·`oblige`·`issue`처럼 뜻이 많고 구용구가 붙는 단어를 위주로 — 이 앱의 분산가치(Sense 단위 복습)를 첫 화면에서 보여주는 것이 목적이다. 직접 고른 것이므로 라이선스 문제가 없다. 예문도 직접 쓴다.
- **Audio**: 서버 TTS를 호출해 단어별 mp3를 생성하고 **영구 캐시**한다. 한 단어는 한 번만 생성하므로 500단어도 500KB다. 비용은 무시할 수준이고, 캐시된 단어는 오프라인에서 재생된다.
- 사용자 정의 Sense는 출처 배지로 구분해 사전 조회 결과와 섞이지 않게 한다.
- Sense 정리는 `archived_at`으로 한다. definition이 빈 Sense는 못 만들며, 그 이상은 막지 않는다 — 쓰레기가 생기면 그 덱을 버리면 된다.

## Phase 로드맵

### Phase 1 — 도메인 (서버·UI 없음) — **완료**

가치 검증이 목표였다. 스케줄러가 장기 기억에 실제로 좋은지 수치로 확인하기 전에는 아무것도 쌓지 않는다.

1. ✅ `fsrs`가 `wasm32-unknown-unknown`에서 빌드되는지 확인. `getrandom`이 컴파일을 막았고 `wasm_js` shim으로 해결했다 ([ADR-0008](./adr/0008-getrandom-wasm-shim.md))
2. ✅ `voca-domain`: `Word`·`Sense`·`Card`·`Deck`·`Rating`·`Review` 타입, `Scheduler` 래퍼, XP/Level/Retention Preset 규칙, `Streak` 계산
3. ✅ `voca-sim` 시뮬레이터. learning step을 0으로 고정해 실제 스케줄러와 모델을 맞췄다 ([ADR-0009](./adr/0009-no-learning-steps-pure-fsrs-6.md))
4. ✅ Retention Preset 3개의 예상 일일 부하 측정 → 프리셋 값과 `daily_goal` 기본값 확정 (위 표)

**산출물은 스키마가 아니라 숫자였다.** 그 숫자가 `decks.daily_goal` 기본값(20)과 프리셋 화면의 문구를 결정했다.

남은 것: `Scheduler`에 사용자별 파라미터를 주입하는 경로. `review_log`를 모아 네이티브 잡에서 `compute_parameters`를 돌린 뒤 `Scheduler::with_parameters`로 교체한다. Phase 4 이후.

### Phase 2 — 저장소와 서버

1. ✅ `voca-store` seam. 테이블 CRUD가 아니라 **도메인 연산** 단위다 ([ADR-0012](./adr/0012-store-seam-is-domain-operations-not-table-crud.md))
2. `voca-store-sqlite`: sqlx + 마이그레이션. 배포는 단일 VPS의 SQLite 파일이다 ([ADR-0010](./adr/0010-server-db-is-sqlite-on-a-single-vps.md))
3. `voca-dict`: 외부 사전 API 클라이언트 + 캐시
4. `voca-server`: axum + 세션 쿠키 인증(argon2) + Leptos SSR 골격
5. 시드 단어 로드 (빈도 상위 + 연어)

`./scripts/check-domain-boundaries.sh`를 CI에 걸어 두 의존 규칙이 깨지지 않게 한다.

### Phase 2 결정 사항 — 저장소 규칙

| 주제 | 결정 |
|---|---|
| 복습 큐 정렬 | 회수 가능성(`fsrs::current_retrievability`) 낮은 순. `new_per_day`는 별도 한도 |
| 동시 Review | `WHERE rev = ?` 낙관적 동시성. 충돌하면 **트랜잭션을 되감고 재시작** |
| 신규 일일 한도 | `decks.new_per_day`. `daily_goal`과 독립 |
| soft delete | 평시 조회에서 걸러내고, `changes_since`에서만 tombstone으로 노출 |
| 새로 만든 행의 `rev` | **1**. 0이면 동기화 트리거가 안 타 새 클라이언트가 그 행을 못 받는다 |
| 계정·세션 interface | `Store`와 분리. `Accounts`·`Sessions`. 바뀌는 이유가 다르다 |
| 인증 위치 | 규칙은 프레임워크 무-depend. HTTP 는 얇은 어댑터 ([ADR-0013](./adr/0013-auth-rules-are-separate-from-http.md)) |

### Phase 2 진행 상황

1. ✅ `voca-store` seam — 도메인 연산 단위 ([ADR-0012](./adr/0012-store-seam-is-domain-operations-not-table-crud.md))
2. ✅ `voca-store-sqlite` — 스키마, `submit_review` 트랜잭션, 큐 정렬, 대시보드, 동기화
3. ✅ `Accounts`·`Sessions` — argon2id, 토큰 해시 저장, 계정 열거 방어
4. ✅ `voca-server` — 인증 규칙 (`auth.rs`) + 비밀번호 (`password.rs`)
5. ⬜ axum 라우터 + 쿠키 + CSRF — 규칙은 다 됐고 배선만 남았다
6. ⬜ `voca-dict` — 외부 사전 API + 캐시
7. ⬜ Leptos SSR 골격

### Phase 3 — 웹 UI

대시보드(Streak, 오늘 할 일) → 스터디 화면(4버튼 Rating) → 단어 검색·추가 + 사전 팝업(출처 배지) → 사용자 Sense 편집 → 통계. 전부 [ADR-0002](./adr/0002-ui-must-be-csr-compatible-from-day-one.md)의 제약 아래 작성.

### Phase 4 — 콘텐츠

TTS mp3 생성·캐시, 녹음 후 자기 비교, 시드 단어 목록 확정.

### Phase 5 — 동기화와 클라이언트

서버 중재 동기화 엔진 → `src-tauri` 데스크톱 → Tauri 모바일.

## 열린 항목

7개 미해결 질문은 모두 닫혔다. 아래는 **막히지 않지만 미룬 것**이다.

1. **마일스톤의 실제 목록.** "단어 100개, 스트릭 100일"은 예시다. 무엇을 몇 개 기준으로 삼을지는 Phase 3 UI와 함께 정한다. 기준을 나중에 바꾸면 이미 달성한 사용자에게 마이그레이션이 필요하다.
2. **레벨 제목의 개수와 경계.** 1~50은 직접 쓰되, 그 이상은 어떻게 나눌지. 무한히 늘리면 이름이 고갈된다.
3. **외부 사전 API의 장애 시나리오.** dictionaryapi.dev가 죽거나 한도 초과하면 신규 단어 조회가 안 된다. 시드 단어만으로는 부족하다. 캐시 TTL과 사용자에게 보여줄 상태가 정해지면 그때.
4. **`voca-ui`가 생기면 제목 이름과 프리셋 이름을 거기서 로드한다.** 지금은 도메인에 넣지 않았다. 도메인에 한국어 표시 문자열을 두는 것은 계층 역전이므로.

## 이전에 닫힌 질문

2026-09 기준 아래를 결정했다. 각 항목의 근거는 해당 ADR에 있다.

| 주제 | 결정 | 근거 |
|---|---|---|
| `example` Sense의 Word | Card 앞면만 달라지고 `word_id`는 NOT NULL | [ADR-0011](./adr/0011-card-front-is-determined-by-sense-kind.md) |
| 서버 DB | 단일 VPS + SQLite 파일 (WAL) | [ADR-0010](./adr/0010-server-db-is-sqlite-on-a-single-vps.md) |
| 성숙 단어를 잊은 직후 | 가만히 두고 안내만 | [ADR-0009](./adr/0009-no-learning-steps-pure-fsrs-6.md) |
| 사용자 Sense 정리 | `definition` 필수 + `archived_at` | [ADR-0006](./adr/0006-sense-covers-more-than-dictionary-definitions.md) |
| 시드 단어 선정 | 영어 빈도 상위 + 연어 많은 단어 | [ADR-0007](./adr/0007-word-data-is-fetched-at-runtime-not-bundled.md) |
| Audio | 서버 TTS + 단어별 영구 캐시 | — |
| Level 보상 | 제목 이름 + 마일스톤 축하 | — |
| 복습 큐 정렬 | 회수 가능성 낮은 순 | [ADR-0012](./adr/0012-store-seam-is-domain-operations-not-table-crud.md) |
| 동시 Review | 낙관적 동시성 + 재계산 | [ADR-0012](./adr/0012-store-seam-is-domain-operations-not-table-crud.md) |
| 신규 일일 한도 | `decks.new_per_day` | [ADR-0012](./adr/0012-store-seam-is-domain-operations-not-table-crud.md) |

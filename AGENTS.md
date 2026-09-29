# AGENTS.md

Voca Memory — Rust 워크스페이스. 설계 근거는 `docs/design.md`, 용어는 `CONTEXT.md`,
개별 결정은 `docs/adr/`. **이 셋이 진짜 규칙이다.** 코드가 문서와 다르면 코드를
고치거나 문서를 같이 갱신한다 — 어느 한쪽만 조용히 바꾸지 않는다.

## 시작 전 반드시 아는 사실

- **sqlx는 rustc 1.94를 요구한다.** `Cargo.lock`의 sqlx 0.9.0(sqlx-core·macros·
  macros-core·sqlite 전부)이 `rust-version = 1.94`를 선언한다. 워크스페이스
  `rust-version`은 1.88이지만 이것은 **최소** 사양이므로 1.94+ 툴체인이면 그대로
  돈다. 이 머신은 `rustup update stable`로 1.98.1까지 올려뒀다 — 그 전에는
  `cargo build -p voca-server`가 sqlx를 요구한다는 메시지와 함께 시작부터 실패한다.
  롤백하려면 `cargo update sqlx --precise 0.8.6` (lockfile 변경, 0.8/0.9 API 차이
  확인 필요)이라 저장소가 더러워진다. 툴체인을 올리는 쪽이 낫다.
  `voca-domain`·`voca-store`·`voca-ui`·`voca-sim`은 sqlx가 없어 1.88에서도 돈다.
- **CI가 없다.** `.github/`도 린트 설정도 없다. `scripts/check-domain-boundaries.sh`가
  유일한 게이트이고, **지금 실패한다** — `wasm32-unknown-unknown` 타깃이 없어서
  (`rustup target add wasm32-unknown-unknown`).
- `cargo-leptos`가 설치돼 있지 않다. `web/` 디렉터리도 아직 없다
  (`docs/design.md`의 아키텍처 그림이 미래형이다). SSR은 지금 `voca-server`가 직접
  `voca_ui::render_page`로 그린다.

## 명령

```bash
# 단일 크레이트
cargo test -p voca-domain
cargo test -p voca-store-sqlite --test submit_review     # 통합 테스트 파일 단위
cargo test -p voca-server --test auth health_answers     # 이름으로 한 건
cargo test --workspace

./scripts/check-domain-boundaries.sh    # 의존 규칙 + wasm 컴파일 + async 누출
./scripts/smoke.sh [포트]               # 12단계 라이브 왕복 (curl + python3)
```

- `scripts/smoke.sh`는 **미리 빌드된 `target/debug/voca-server` 를 필요로 한다**
  (`cargo build` 먼저). `jq`가 아니라 `python3` 로 JSON을 읽는다.
- 테스트는 전부 오프라인이다. 외부 사전에 실제 네트워크로 나가지 않는다 —
  `crates/voca-server/tests/dictionary.rs`가 테스트 안에서 가짜 axum 서버를 띄운다.

## 레이어 규칙 (역방향 금지)

`voca-domain` ← `voca-store` ← `voca-store-sqlite` ← `voca-dict` / `voca-ui` / `voca-server`

- `voca-domain`의 직접 의존 allowlist는 `fsrs`, `serde`, `time`, `uuid`, `getrandom`뿐.
  새 의존이 필요하면 **`docs/design.md`의 의존 규칙 표를 먼저 고친다.** 스크립트가
  그것을 요구한다.
- `voca-domain`은 I/O 없음, `async` 없음, 결정적. `next_states()`만 호출한다
  (`getrandom` wasm shim은 컴파일 타임에만 존재 — `docs/adr/0008`).
- `voca-store`는 **테이블 CRUD가 아니라 도메인 연산** seam이다. `submit_review` 하나가
  `review_log`·`card_states`·`streaks`·`xp_events`를 한 트랜잭션으로 갱신한다.
  호출부가 이걸 직접 조합하지 않는다.
- `voca-server`는 얇은 어댑터다. 규칙이 `routes.rs`에 생기면 그건 설계 위반이다.

## 구현에서 놓치기 쉬운 함정

- **trait는 `async fn`이 아니라 RPITIT** (`-> impl Future<Output = ..> + Send`).
  `#[async_trait]`도 금지 — axum 핸들러에서 `Send`가 깨지거나 박싱 비용이 생긴다.
- **`sqlx::query!` 매크로를 쓰지 않는다.** `DATABASE_URL`과 빌드 타임 DB가 필요해져
  로컬 개발과 CI가 묶인다. `query_as` + `FromRow`만. 스키마 정합성은 통합 테스트가 지킨다.
- **SQLite `:memory:` 는 연결마다 별개 DB다.** `open_in_memory()`가 `max_connections(1)`
  로 묶어 둔 이유다. 풀을 늘리면 마이그레이션 스키마가 사라진다.
- **PRAGMA는 연결마다 설정해야 한다.** 마이그레이션 SQL에 써도 그 마이그레이션을 실행한
  연결 하나에만 적용된다. `SqliteStore::open`이 강제한다.
- **새 행의 `rev` 는 1이다 (0이 아니다).** 0이면 동기화 트리거가 안 타 새 클라이언트가
  그 행을 못 받는다. `revision.next()` 는 `voca-store/src/revision.rs` 한 군데서만 일어난다.
- **soft delete**는 평시 조회에서 걸러내고 `changes_since`에서만 tombstone으로 노출한다.
  호출자는 `deleted_at`의 존재를 몰라야 한다.
- `StoreError` 분류는 작게 유지한다: `NotFound` / `Invalid(&'static str)` / `Integrity` /
  `ConflictExhausted` / `Transient` / `Unavailable`. 상세가 필요하면 로그에 남긴다.
  재시도 가능성은 `is_retryable()` 한 곳에서만.
- 낙관적 동시성 충돌은 트랜잭션을 되감고 재시도한다 (`MAX_CONFLICT_RETRIES = 8`).
- 로그인 판정에서 계정 없음과 비밀번호 오류는 **같은 것**으로 돌려준다.

## 테스트 관례

- `tests/common/mod.rs`의 `Harness`가 라우터를 **소켓 없이 `oneshot` 으로 직접 부른다.**
  새 HTTP 테스트는 이걸 쓴다. 진짜 TCP 왕복은 `tests/live_socket.rs` 하나뿐이고
  거기는 전송 계층(상태 줄·헤더 구분)만 본다.
- 테스트 함수 이름은 **영어 완전 문장**이다: `an_empty_allow_list_blocks_every_mutation`.
  한국어로 지으면 안 된다.
- 순수 규칙은 `src/*.rs` 안의 `#[cfg(test)] mod tests`에 둔다. crate 밖으로 빼지 않는다.
- HTTP 테스트는 `Harness::with_origins` / `with_dictionary`로 조립한다.
  `Config`를 직접 만드는 것도 허용된다.

## 환경 변수

`crates/voca-server/src/config.rs`가 유일한 출처. 설정 파일 형식은 없다.

| 변수 | 기본 | 비고 |
|---|---|---|
| `VOCA_DB_PATH` | **필수** | 기본값을 두지 않는다 |
| `VOCA_HOST` / `VOCA_PORT` | `127.0.0.1` / `3000` | `0.0.0.0`이 아니다 |
| `VOCA_ALLOWED_ORIGINS` | 비어 있음 | **비면 모든 변경 요청이 403** — CSRF가 조용히 죽는다 |
| `VOCA_SECURE_COOKIES` | `true` | 로컬 HTTP에서만 `0` |
| `VOCA_LOG` | `voca_server=info,...` | tracing EnvFilter |

## 언어·문체

- **식별자는 영어, 주석과 문서는 한국어.** 새 모듈은 파일 맨 위 `//!` 한국어 블록으로
  왜 그렇게 했는지 남긴다. "이렇게 안 하면 무엇이 깨지는가"를 쓴다.
- `voca-web/Cargo.toml`의 `output-name`은 서버의 `page.rs` `SCRIPT_SRC`
  (`/pkg/voca-web.js`)와 같아야 한다. 어긋나면 스크립트가 404가 되고 페이지가
  **조용히** interactivity를 잃는다.
- XP 산식(`Again`=0 / `Hard`=1 / `Good`=2 / `Easy`=4), `level = floor(sqrt(xp/200)) + 1`,
  Retention Preset 3개, `daily_goal` 기본 20은 **고정값**이다. 바꾸면 이미 쌓인
  `xp_events` 해석이 갈라진다 — 되돌릴 때는 마이그레이션 계획이 따라온다.

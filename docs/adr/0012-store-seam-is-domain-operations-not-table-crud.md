# 저장소 seam은 테이블이 아니라 도메인 연산 단위다

`voca-store`의 `Store` 트레잇은 `insert`·`update`·`delete` 같은 **테이블 CRUD가 아니다.** `submit_review`, `study_queue`, `dashboard`처럼 **도메인이 시키는 일**을 인터페이스에 둔다.

## 왜

`CardRepository { insert, update, delete, find_by_id }`는 삭제해도 복잡성이 사라지지 않는다. 그 CRUD는 sqlx가 이미 준 것이다. 그런 인터페이스는 얕고, 얕은 모듈은 호출부로 복잡성을 흩뿌린다.

진짜 비용은 한 곳에 몰려 있다. `submit_review` 하나가 다음을 **한 트랜잭션**으로 해야 한다.

- `review_log` 삽입 (append-only)
- `card_states` 갱신 + `rev` 증가
- 낙관적 동시성 충돌 시 재조회 후 스케줄 재계산
- `streaks` 재계산
- `xp_events` 삽입

이걸 호출부가 네 개의 메서드로 조합하게 두면, 조합을 잘못한 곳이 하나라도 생긴다. 스케줄러가 순수 함수라서 이 재계산은 I/O 없이 트랜잭션 안에서 끝난다 — 경계를 오가지 않는다.

## `study_queue`가 정렬 정책까지 가지는 이유

`due_at` 오름차순으로 정렬하고 신규 한도를 Store 밖에서 처리한다면, "순서가 틀렸다"와 "오늘 너무 많이 나온다"가 두 군데서 각각 결정된다. 둘 다 스케줄링 결정이고, 엇갈리면 사용자가 이유를 알 수 없는 밀도를 본다.

그래서 이 인터페이스 하나에 **정렬(회수 가능성 낮은 순) + 신규 일일 한도**가 들어 있다. 호출부는 Card를 몇 개 원한다고만 말한다.

## `Preview`와 `Level`을 반환하는 이유

`submit_review`는 반영 결과만 주지 않고 4개 Rating의 도착 지점과 갱신된 Level까지 준다. 호출부가 이걸 모으려고 두 번째 조회를 하면, 그 사이에 사용자가 다음 Review를 눌러 숫자가 어긋난 화면이 뜬다. 계산을 한 곳에 모으는 것은 locality 문제이기도 하다.

## `async fn`이 아니라 RPITIT

`async fn`을 trait에 쓰면 반환 future에 `Send`를 선언할 수 없다. axum 핸들러에서 쓰면 "future is not `Send`"로 막힌다. 그래서 `impl Future<Output = ..> + Send`를 명시한다. `#[async_trait]`로 감싸면 `Box<dyn Future>` 할당이 생기므로 그것도 쓰지 않는다.

## soft delete는 평시 경로에서 보이지 않는다

일반 조회는 `deleted_at`이 찍힌 행을 걸러낸다. 호출자는 그 열의 존재를 모른다. 동기화 경로(`changes_since`)만 tombstones을 그대로 노출한다. 그래야 soft delete가 동기화 규칙이면서 동시에 동기화 구현의 내부 일이 된다.

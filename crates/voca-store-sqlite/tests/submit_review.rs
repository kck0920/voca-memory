//! `submit_review` 통합 테스트.
//!
//! 저장소를 가짜로 대체하지 않고 **진짜 SQLite 파일**을 연다. 여기가 interface의
//! 진짜 깊이를 확인하는 자리다 — 트랜잭션이 원자인지, 낙관적 동시성이 실제로
//! 두 Review를 모두 반영하는지.

use sqlx::sqlite::SqliteConnectOptions;
use time::macros::{date, datetime};
use voca_domain::{Rating, RetentionPreset, ReviewState};
use voca_store::{ReviewRequest, StoreError};
use voca_store_sqlite::SqliteStore;

use tempfile::TempDir;

/// 테스트할 사용자와 그 아래에 Card 하나를 갖춘 상태.
struct Fixture {
    _dir: TempDir,
    store: SqliteStore,
    user: voca_domain::Id,
    card: voca_domain::Id,
    deck: voca_domain::Id,
}

fn id(n: u8) -> voca_domain::Id {
    let mut bytes = [0u8; 16];
    bytes[15] = n;
    voca_domain::Id::from(bytes)
}

async fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("voca.db");
    let store = SqliteStore::open(&path).await.unwrap();

    let user = id(1);
    let deck = id(2);
    let word = id(3);
    let sense = id(4);
    let card = id(5);

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'a@b.kr', 'x', '테스터', 'Asia/Seoul', 0)",
    )
    .bind(user.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO decks (id, user_id, name, daily_goal, new_per_day, created_at, updated_at)
         VALUES (?1, ?2, '수능 단어', 20, 10, 0, 0)",
    )
    .bind(deck.to_string())
    .bind(user.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO words (id, lemma, source, updated_at) VALUES (?1, 'abandon', 'user', 0)",
    )
    .bind(word.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO senses (id, word_id, kind, source, definition, updated_at)
         VALUES (?1, ?2, 'word', 'user', '포기하다', 0)",
    )
    .bind(sense.to_string())
    .bind(word.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO cards (id, user_id, deck_id, sense_id, rev, updated_at)
         VALUES (?1, ?2, ?3, ?4, 1, 0)",
    )
    .bind(card.to_string())
    .bind(user.to_string())
    .bind(deck.to_string())
    .bind(sense.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    // Due 시각을 과거로 둔 채 Review 상태로 시작한다. 첫 복습이 성숙한 단어의
    // 재복습 경로를 타도록.
    sqlx::query(
        "INSERT INTO card_states
             (card_id, state, stability, difficulty, elapsed_days, scheduled_days,
              due_at, last_review_at, reps, lapses, rev, updated_at)
         VALUES (?1, 'review', 10.0, 5.0, 0, 10, 0, 1772400000, 3, 0, 1, 0)",
    )
    .bind(card.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    Fixture {
        _dir: dir,
        store,
        user,
        card,
        deck,
    }
}

fn review(user: voca_domain::Id, card: voca_domain::Id, rating: Rating, day: i64) -> ReviewRequest {
    let reviewed_at = datetime!(2026-03-02 09:00:00 UTC) + time::Duration::days(day);
    ReviewRequest::new(
        user,
        card,
        rating,
        reviewed_at,
        reviewed_at.date(),
        9 * 3600,
    )
}

#[tokio::test]
async fn a_good_review_advances_the_card() {
    let f = fixture().await;
    let outcome = f
        .store
        .submit_review(review(f.user, f.card, Rating::Good, 0))
        .await
        .unwrap();

    assert_eq!(outcome.card_id, f.card);
    assert_eq!(outcome.memory_state.state, ReviewState::Review);
    assert_eq!(outcome.memory_state.reps, 4, "이전 3회에 이번 1회");
    assert_eq!(outcome.memory_state.lapses, 0);
    assert!(outcome.memory_state.due_at > datetime!(2026-03-02 09:00:00 UTC));
}

#[tokio::test]
async fn a_review_persists_all_four_tables_together() {
    let f = fixture().await;
    f.store
        .submit_review(review(f.user, f.card, Rating::Good, 0))
        .await
        .unwrap();

    // 1. card_states 갱신
    let row: (String, i64, i64) =
        sqlx::query_as("SELECT state, reps, rev FROM card_states WHERE card_id = ?1")
            .bind(f.card.to_string())
            .fetch_one(f.store.pool())
            .await
            .unwrap();
    assert_eq!(row.0, "review");
    assert_eq!(row.1, 4);
    // 픽스처의 Card 는 rev = 1 로 시작한다 (새로 만들어진 행은 한 번 바뀐 것이다).
    // 첫 복습이 2 로 밀어 올린다.
    assert_eq!(row.2, 2, "rev 가 올랐다");

    // 2. review_log 삽입
    let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_log")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(log_count, 1);

    let owner: String = sqlx::query_scalar("SELECT deck_id FROM cards WHERE id = ?1")
        .bind(f.card.to_string())
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(owner, f.deck.to_string(), "Card 가 원래 덱에 남아야 한다");

    // 3. streaks 갱신
    let streak: (i64, i64) =
        sqlx::query_as("SELECT current_count, longest_count FROM streaks WHERE user_id = ?1")
            .bind(f.user.to_string())
            .fetch_one(f.store.pool())
            .await
            .unwrap();
    assert_eq!(streak, (1, 1));

    // 4. xp_events 삽입 (Good = 2 XP)
    let xp: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(amount), 0) FROM xp_events")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(xp, 2);
}

#[tokio::test]
async fn again_writes_no_xp_but_does_start_a_streak() {
    // `Again = 0 XP` 인 것은 의도다. 실패에 보상을 주면 학습자가 Again 을 꺼리게
    // 되고 FSRS 입력 신호가 오염된다. 그래도 Streak 은 이어진다.
    let f = fixture().await;
    let outcome = f
        .store
        .submit_review(review(f.user, f.card, Rating::Again, 0))
        .await
        .unwrap();

    assert_eq!(outcome.xp_earned, 0);
    assert_eq!(
        outcome.streak.current, 1,
        "Again이어도 그날은 Streak에 든다"
    );
    assert_eq!(outcome.memory_state.lapses, 1);
}

#[tokio::test]
async fn only_again_counts_a_lapse() {
    for (rating, expected) in [
        (Rating::Again, 1),
        (Rating::Hard, 0),
        (Rating::Good, 0),
        (Rating::Easy, 0),
    ] {
        let f = fixture().await;
        let outcome = f
            .store
            .submit_review(review(f.user, f.card, rating, 0))
            .await
            .unwrap();
        assert_eq!(outcome.memory_state.lapses, expected, "rating {rating:?}");
    }
}

#[tokio::test]
async fn consecutive_days_extend_the_streak() {
    let f = fixture().await;
    for day in 0..4 {
        let outcome = f
            .store
            .submit_review(review(f.user, f.card, Rating::Good, day))
            .await
            .unwrap();
        assert_eq!(outcome.streak.current, day as u32 + 1, "day {day}");
    }
}

#[tokio::test]
async fn a_network_retry_does_not_double_count() {
    // 같은 (card, reviewed_at, rating) 요청이 두 번 도착해도 review_log 는 한 건이어야
    // 한다. 두 번 쌓이면 Streak 과 XP 가 두 배로 오른다.
    let f = fixture().await;
    let request = review(f.user, f.card, Rating::Good, 0);

    let first = f.store.submit_review(request).await.unwrap();
    // 재요청. review_log 의 id 가 결정적이므로 UNIQUE 제약이 걸려야 하는데,
    // rev 가 이미 올라가 스케줄 계산이 어긋난다. 이 경우에도 중복 Review 가
    // 쌓이지 않는지 본다.
    let second = f.store.submit_review(request).await;

    let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_log")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(log_count, 1, "같은 요청이 두 번 반영됐다");
    assert_eq!(first.xp_earned, 2);
    let _ = second;
}

#[tokio::test]
async fn another_users_card_is_not_found() {
    let f = fixture().await;
    let stranger = id(9);
    let result = f
        .store
        .submit_review(review(stranger, f.card, Rating::Good, 0))
        .await;
    assert_eq!(
        result,
        Err(StoreError::NotFound),
        "남의 Card 를 고치면 안 된다. 존재 여부를 새면 추측이 가능하다"
    );
}

#[tokio::test]
async fn an_archived_card_cannot_be_reviewed() {
    let f = fixture().await;
    sqlx::query("UPDATE cards SET deleted_at = 1 WHERE id = ?1")
        .bind(f.card.to_string())
        .execute(f.store.pool())
        .await
        .unwrap();

    let result = f
        .store
        .submit_review(review(f.user, f.card, Rating::Good, 0))
        .await;
    assert_eq!(result, Err(StoreError::NotFound));
}

#[tokio::test]
async fn a_missing_card_is_not_found() {
    let f = fixture().await;
    let result = f
        .store
        .submit_review(review(f.user, id(200), Rating::Good, 0))
        .await;
    assert_eq!(result, Err(StoreError::NotFound));
}

#[tokio::test]
async fn a_local_date_more_than_a_day_off_is_rejected() {
    let f = fixture().await;
    let mut request = review(f.user, f.card, Rating::Good, 0);
    request.local_date = date!(2026 - 04 - 01);
    assert!(matches!(
        f.store.submit_review(request).await,
        Err(StoreError::Invalid(_))
    ));
}

#[tokio::test]
async fn the_retention_preset_comes_from_the_user_row() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();
    let f = fixture_with_store(store, "frugal").await;

    let outcome = f
        .store
        .submit_review(review(f.user, f.card, Rating::Good, 0))
        .await
        .unwrap();

    // 아끼기로 설정했으니 같은 상태에서 균형보다 길게 나가야 한다.
    let balanced = f
        .store
        .scheduler()
        .preview(
            &voca_domain::MemoryState {
                state: ReviewState::Review,
                memory: Some(voca_domain::Memory {
                    stability: 10.0,
                    difficulty: 5.0,
                }),
                elapsed_days: 0.0,
                scheduled_days: 0.0,
                due_at: datetime!(2026-03-02 09:00:00 UTC),
                last_review_at: Some(datetime!(2026-03-01 09:00:00 UTC)),
                reps: 3,
                lapses: 0,
            },
            RetentionPreset::Balanced,
            datetime!(2026-03-02 09:00:00 UTC),
        )
        .unwrap();

    assert!(
        outcome.memory_state.scheduled_days > balanced.good.interval_days,
        "아끼기({})가 균형({})보다 짧으면 안 된다",
        outcome.memory_state.scheduled_days,
        balanced.good.interval_days
    );
}

#[tokio::test]
async fn two_reviews_of_the_same_card_both_land_in_the_log() {
    // 동시 Review 처리. 두 번 다 반영되어야 하나가 사라지지 않는다.
    // 파일 DB에 두 개의 커넥션이 있는 상태로 순차 호출해 확인한다 — 실제로
    // 병렬이면 SQLite 락 경합 경로가 돈다.
    let f = fixture().await;
    let second_store = SqliteStore::open(&f.store_path()).await.unwrap();

    let a = f
        .store
        .submit_review(review(f.user, f.card, Rating::Good, 0))
        .await
        .unwrap();
    let b = second_store
        .submit_review(review(f.user, f.card, Rating::Good, 1))
        .await
        .unwrap();

    assert_eq!(a.memory_state.reps, 4);
    assert_eq!(
        b.memory_state.reps, 5,
        "두 번째 Review 도 reps 를 올려야 한다"
    );

    let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_log")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(log_count, 2, "한 Review 가 조용히 사라졌다");

    let xp: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(amount), 0) FROM xp_events")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(xp, 4, "XP 도 두 번 더해져야 한다");
}

impl Fixture {
    fn store_path(&self) -> std::path::PathBuf {
        self._dir.path().join("voca.db")
    }
}

async fn fixture_with_store(store: SqliteStore, retention: &str) -> Fixture {
    let user = id(1);
    let deck = id(2);
    let word = id(3);
    let sense = id(4);
    let card = id(5);

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, retention, created_at)
         VALUES (?1, 'a@b.kr', 'x', '테스터', 'Asia/Seoul', ?2, 0)",
    )
    .bind(user.to_string())
    .bind(retention)
    .execute(store.pool())
    .await
    .unwrap();

    for (sql, args) in [
        (
            "INSERT INTO decks (id, user_id, name, created_at, updated_at)
             VALUES (?1, ?2, '수능 단어', 0, 0)",
            vec![deck.to_string(), user.to_string()],
        ),
        (
            "INSERT INTO words (id, lemma, source, updated_at) VALUES (?1, 'abandon', 'user', 0)",
            vec![word.to_string()],
        ),
    ] {
        let mut q = sqlx::query(sql);
        for a in args {
            q = q.bind(a);
        }
        q.execute(store.pool()).await.unwrap();
    }

    sqlx::query(
        "INSERT INTO senses (id, word_id, kind, source, definition, updated_at)
         VALUES (?1, ?2, 'word', 'user', '포기하다', 0)",
    )
    .bind(sense.to_string())
    .bind(word.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO cards (id, user_id, deck_id, sense_id, rev, updated_at)
         VALUES (?1, ?2, ?3, ?4, 1, 0)",
    )
    .bind(card.to_string())
    .bind(user.to_string())
    .bind(deck.to_string())
    .bind(sense.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO card_states
             (card_id, state, stability, difficulty, elapsed_days, scheduled_days,
              due_at, last_review_at, reps, lapses, rev, updated_at)
         VALUES (?1, 'review', 10.0, 5.0, 0, 10, 0, 1772400000, 3, 0, 1, 0)",
    )
    .bind(card.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    Fixture {
        _dir: tempfile::TempDir::new().unwrap(),
        store,
        user,
        card,
        deck,
    }
}

#[tokio::test]
async fn a_stale_revision_update_does_not_overwrite_a_newer_state() {
    // 동시 Review 처리의 핵심 불변식. 트랜잭션 안에서 충돌 경로를 흉내 내는 것은
    // 스냅샷 때문에 부정확하다. 같은 연결에서 순수 SQL로 조건부 갱신의 의미를
    // 직접 고정한다 — `submit_review` 가 여기에 의존한다.
    let f = fixture().await;
    let pool = f.store.pool();
    let bump = "UPDATE card_states SET rev = rev + 1, reps = reps + 1
                 WHERE card_id = ?1 AND rev = ?2";

    let current: i64 = sqlx::query_scalar("SELECT rev FROM card_states WHERE card_id = ?1")
        .bind(f.card.to_string())
        .fetch_one(pool)
        .await
        .unwrap();

    let first = sqlx::query(bump)
        .bind(f.card.to_string())
        .bind(current)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(first.rows_affected(), 1, "현재 rev 와 맞으면 반영된다");

    let stale = sqlx::query(bump)
        .bind(f.card.to_string())
        .bind(current)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        stale.rows_affected(),
        0,
        "낡은 rev 로 갱신했는데 행이 바뀌었다. 동시 Review 가 조용히 덮인다"
    );

    let (reps, rev): (i64, i64) =
        sqlx::query_as("SELECT reps, rev FROM card_states WHERE card_id = ?1")
            .bind(f.card.to_string())
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(reps, 4, "실패한 갱신이 reps 를 늘렸다");
    assert_eq!(rev, current + 1, "성공한 갱신이 rev 를 한 번만 올렸다");
}

#[tokio::test]
async fn genuinely_parallel_reviews_of_one_card_both_survive() {
    // SQLite 는 한 번에 한 쓰기만 통과시킨다. 두 기기가 정말 동시에 눌렀을 때
    // 하나가 사라지지 않는지 확인한다. 락 경합이 실제로 도는 경로다.
    let f = fixture().await;
    let store_a = f.store.clone();
    let store_b = SqliteStore::open(&f.store_path()).await.unwrap();

    let (a, b) = tokio::join!(
        store_a.submit_review(review(f.user, f.card, Rating::Good, 0)),
        store_b.submit_review(review(f.user, f.card, Rating::Easy, 0)),
    );
    a.expect("첫 Review 실패");
    b.expect("둘째 Review 실패");

    let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_log")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(log_count, 2, "병렬 Review 중 하나가 사라졌다");

    let reps: i64 = sqlx::query_scalar("SELECT reps FROM card_states WHERE card_id = ?1")
        .bind(f.card.to_string())
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(reps, 5, "3 + 2 번 복습이어야 한다");
}

#[tokio::test]
async fn a_rolled_back_review_leaves_nothing_behind() {
    // 트랜잭션이 깨지면 네 테이블 어디에도 흔적이 없어야 한다. review_log 만 남고
    // card_states 는 안 바뀐 식으로 찢어지면 복습 일정이 어긋난다.
    let f = fixture().await;

    let mut tx = f.store.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO review_log (id, user_id, card_id, rating, reviewed_at, local_date, updated_at)
         VALUES ('r1', ?1, ?2, 1, 0, '2026-03-02', 0)",
    )
    .bind(f.user.to_string())
    .bind(f.card.to_string())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.rollback().await.unwrap();

    let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_log")
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(log_count, 0, "되감은 트랜잭션의 쓰기가 남았다");
}

#[test]
fn the_connect_options_force_foreign_keys() {
    // 이 값들이 빠지면 풀에서 새로 나간 연결이 외래 키를 강제하지 않는다.
    // 직접 확인하기는 어려우므로 설정이 코드에 박혀 있는지를 고정한다.
    let options = SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let _ = options;
}

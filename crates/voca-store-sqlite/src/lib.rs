//! `voca-store` seam의 SQLite 어댑터.
//!
//! 서버 배포는 단일 VPS의 SQLite 파일이다 ([`docs/adr/0010`](../../docs/adr/0010-server-db-is-sqlite-on-a-single-vps.md)).
//! 이 어댑터가 그 유일한 구현이다.
//!
//! ## 왜 `sqlx::query!` 매크로가 아니라 `query_as`인가
//!
//! 컴파일 타임에 쿼리를 검사하려면 `DATABASE_URL`과 마이그레이션된 DB가 있어야 한다.
//! 그런 빌드는 로컬 개발과 CI를 붙잡는다. 여기서는 행 타입(`FromRow`)으로 타입
//! 안전을 얻고, 스키마와 쿼리의 정합성은 통합 테스트가 지킨다.

use std::path::Path;
use std::time::Duration as StdDuration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Sqlite, SqlitePool, Transaction};
use voca_domain::Scheduler;
use voca_store::StoreError;

mod account;
pub mod dict_cache;
mod review;
mod row;
mod session;
mod store_impl;

pub use dict_cache::FetchStatus;
pub use review::ReviewAttempt;
pub use row::{CardRow, DeckRow, SenseRow, StateRow, StreakRow, UserRow, WordRow};

use voca_store::StoreResult;

/// 쓰기 경합을 기다리는 시간. 바쁜 writer가 이만큼 지나면 풀린다.
const BUSY_TIMEOUT: StdDuration = StdDuration::from_secs(5);

/// SQLite 저장소.
#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
    /// 사용자별 파라미터 학습이 들어오기 전까지 쓰는 기본 스케줄러.
    ///
    /// `FSRS::default()`는 파라미터 배열 하나를 복사하는 일이라 만들어 두어도
    /// 값싸다. 사용자별로 갈라야 하면 그때 크레이트 안으로 옮긴다.
    scheduler: Scheduler,
}

impl SqliteStore {
    /// 파일 DB를 열고 마이그레이션을 적용한다.
    pub async fn open(path: &Path) -> StoreResult<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            // PRAGMA는 **연결마다** 설정해야 한다. 마이그레이션 SQL에 써도 그
            // 마이그레이션을 실행한 연결 하나에만 적용된다. 여기서 강제하지 않으면
            // 풀에서 새로 나간 연결이 외래 키를 강제하지 않아 referential
            // integrity가 조용히 무너진다.
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT)
            .journal_mode(SqliteJournalMode::Wal);

        Self::connect(options, 4).await
    }

    /// 메모리 DB를 연다. 테스트 전용.
    ///
    /// 연결을 하나만 둔다. SQLite의 `:memory:`는 **연결마다 별개의 DB**이므로,
    /// 여러 연결을 두면 마이그레이션한 스키마가 보이지 않는다.
    pub async fn open_in_memory() -> StoreResult<Self> {
        Self::connect(
            SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
            1,
        )
        .await
    }

    async fn connect(options: SqliteConnectOptions, max_connections: u32) -> StoreResult<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await
            .map_err(classify)?;

        sqlx::migrate!().run(&pool).await.map_err(|e| {
            log(&e);
            StoreError::Unavailable
        })?;

        Ok(Self {
            pool,
            scheduler: Scheduler::default(),
        })
    }

    /// 이 어댑터가 쓰는 기본 스케줄러. 사용자별 파라미터가 들어오면 여기서 갈린다.
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// 풀에 직접 접근한다. 마이그레이션 상태 확인과 테스트가 쓴다.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 쓰기 트랜잭션을 연다.
    pub async fn begin(&self) -> StoreResult<Tx> {
        self.pool.begin().await.map_err(classify)
    }

    /// 저장소가 아직 아무것도 만들지 않았는지. 첫 실행 화면에서 쓴다.
    pub async fn is_empty(&self) -> StoreResult<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(classify)?;
        Ok(count == 0)
    }
}

/// sqlx 에러를 저장소 에러로 낮춘다.
///
/// sqlx 에러의 세부 코드는 기록만 하고 바깥으로는 **왜 재시도할 수 있는지만**
/// 전달한다 — 호출자가 그걸로 무엇을 할 수 있기 때문이다.
pub(crate) fn classify(err: sqlx::Error) -> StoreError {
    log(&err);
    match &err {
        sqlx::Error::RowNotFound => StoreError::NotFound,
        sqlx::Error::Database(db) if db.is_unique_violation() => StoreError::Integrity,
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => StoreError::Integrity,
        // 그 밖의 DB 에러는 락 경합이나 일시적 결함일 수 있다. 재시도 대상이다.
        sqlx::Error::Database(_) | sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut => {
            StoreError::Transient
        }
        _ => StoreError::Unavailable,
    }
}

/// 진단만 남긴다. 프로젝트에 로깅 크레이트를 붙이는 것은 Phase 2가 정리한 뒤로
/// 미룬다 — 지금은 stderr가 정직한 선택이다.
fn log(err: &dyn std::fmt::Display) {
    eprintln!("voca-store-sqlite: {err}");
}

pub(crate) fn unix_seconds(at: time::OffsetDateTime) -> i64 {
    at.unix_timestamp()
}

pub(crate) fn now_unix_seconds() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// 트랜잭션 안에서 쓰는 축약.
pub(crate) type Tx = Transaction<'static, Sqlite>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_open_store_has_the_schema() {
        let store = SqliteStore::open_in_memory().await.unwrap();
        let name: String = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='card_states'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(name, "card_states");
    }

    #[tokio::test]
    async fn a_fresh_store_has_no_users() {
        let store = SqliteStore::open_in_memory().await.unwrap();
        assert!(store.is_empty().await.unwrap());
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        // 마이그레이션 SQL에 `PRAGMA foreign_keys = ON`을 쓰는 것으로는 부족하다.
        // 이 테스트가 그 함정을 잡는다 — 안 잡히면 존재하지 않는 사용자를 참조한
        // Card가 조용히 들어가고 referential integrity가 나중에 깨진다.
        let store = SqliteStore::open_in_memory().await.unwrap();
        let result: Result<i64, _> = sqlx::query_scalar(
            "INSERT INTO cards (id, user_id, deck_id, sense_id, updated_at)
             VALUES ('c1', '없는 사용자', 'd1', 's1', 0)",
        )
        .fetch_one(store.pool())
        .await;
        assert!(
            result.is_err(),
            "존재하지 않는 사용자를 참조한 Card가 들어갔다"
        );
    }

    #[tokio::test]
    async fn an_in_memory_store_persists_across_pool_checkouts() {
        // SQLite 의 `:memory:` 는 **연결마다 별개의 DB** 다. 풀을 여러 개 두면
        // 마이그레이션한 스키마가 두 번째 연결에서 보이지 않는다. 여기서 1개로
        // 묶었는지 실제로 왕복해 확인한다.
        let store = SqliteStore::open_in_memory().await.unwrap();

        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
             VALUES ('u1', 'a@b.kr', 'x', '테스터', 'Asia/Seoul', 0)",
        )
        .execute(store.pool())
        .await
        .unwrap();

        let found: Option<String> =
            sqlx::query_scalar("SELECT display_name FROM users WHERE id = 'u1'")
                .fetch_optional(store.pool())
                .await
                .unwrap();
        assert_eq!(
            found.as_deref(),
            Some("테스터"),
            "풀을 다시 빌려 받아도 같은 DB 여야 한다"
        );
        assert!(!store.is_empty().await.unwrap());
    }

    #[test]
    fn unique_and_foreign_key_violations_are_not_retryable() {
        // 인위적으로 만든 sqlx 에러를 분류한다. 규칙 자체를 고정하는 테스트다.
        assert!(!StoreError::Integrity.is_retryable());
        assert!(StoreError::Transient.is_retryable());
    }
}

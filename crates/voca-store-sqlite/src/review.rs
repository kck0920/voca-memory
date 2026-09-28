use voca_domain::{
    LevelProgress, MemoryState, Preview, RetentionPreset, ReviewState, Streak, xp_for, xp_progress,
};
use voca_store::{ReviewOutcome, ReviewRequest, StoreError, StoreResult};

use crate::row::{StateRow, StreakRow};
use crate::{SqliteStore, Tx, classify, now_unix_seconds, unix_seconds};

/// 한 번의 적용 시도 결과.
pub enum ReviewAttempt {
    /// 이 기기가 반영을 끝냈다.
    Applied(Box<AppliedReview>),
    /// 다른 writer가 그 사이에 반영했다. **트랜잭션을 버리고 처음부터 다시 해야 한다.**
    Conflict,
}

pub struct AppliedReview {
    pub memory_state: MemoryState,
    pub preview: Preview,
    pub prev_stability: Option<f32>,
    pub next_stability: Option<f32>,
}

/// 트랜잭션 재시도 상한. 충돌과 일시적 실패를 모두 이 안에서 소진한다.
///
/// 충돌은 같은 `(card, reviewed_at, rating)`에 대한 재요청이 두 번 겹친 경우라
/// 실제로는 한두 번이면 끝난다. 넉넉하게 둔다.
const TOTAL_RETRIES: usize = (voca_store::MAX_CONFLICT_RETRIES as usize) + 3;

impl SqliteStore {
    /// Review 하나를 원자적으로 반영한다.
    ///
    /// 한 트랜잭션에서: `card_states` 갱신 → `review_log` 삽입 → `streaks` 재계산 →
    /// `xp_events` 삽입. **이 넷을 호출부가 조합하지 않는다** —
    /// [`docs/adr/0012`](../../docs/adr/0012-store-seam-is-domain-operations-not-table-crud.md).
    ///
    /// ## 동시 Review 처리
    ///
    /// 두 가지를 함께 쓴다.
    ///
    /// 1. `WHERE rev = ?` 조건부 갱신. 다른 writer가 먼저 반영했으면 0행이다.
    /// 2. 트랜잭션 전체를 되감고 재시작.
    ///
    /// **충돌을 같은 트랜잭션 안에서 재조회로 해결하지 않는 게 중요하다.** SQLite의
    /// 트랜잭션은 첫 읽기에서 스냅샷을 고정하므로, 그 안에서 다시 읽어도 항상 낡은
    /// 값을 본다. 재조회를 반복하면 영원히 `rev`가 맞지 않는다. 되감고 새로 시작해야
    /// 새 스냅샷을 얻는다.
    ///
    /// 부수적으로, 실제 SQLite에서는 rev 조건에 닿기 전에 `SQLITE_BUSY_SNAPSHOT`이
    /// 먼저 나는 경우가 많다. 그건 `StoreError::Transient`로 분류되어 같은 재시도
    /// 경로를 탄다. rev 조건은 그와 별개인 두 번째 방어선이다.
    pub async fn submit_review(&self, request: ReviewRequest) -> StoreResult<ReviewOutcome> {
        request.validate()?;

        let mut last = StoreError::Transient;
        for _ in 0..TOTAL_RETRIES {
            match self.submit_review_once(&request).await {
                Ok(outcome) => return Ok(outcome),
                Err(e) if e.is_retryable() || e == StoreError::ConflictExhausted => {
                    last = e;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    async fn submit_review_once(&self, request: &ReviewRequest) -> StoreResult<ReviewOutcome> {
        let mut tx = self.begin().await?;

        // 충돌이 나면 이 트랜잭션은 버린다. 안쪽 루프는 스키마 이상(한 트랜잭션에서
        // 두 번 갱신)만 방어하고, 진짜 재시작은 바깥 루프가 한다.
        let mut conflict_count: u32 = 0;
        let applied = loop {
            match self.try_apply(&mut tx, request).await? {
                ReviewAttempt::Applied(a) => break a,
                ReviewAttempt::Conflict => {
                    conflict_count += 1;
                    if conflict_count > voca_store::MAX_CONFLICT_RETRIES {
                        return Err(StoreError::ConflictExhausted);
                    }
                }
            }
        };

        let user_id = request.user.to_string();
        let xp_earned = xp_for(request.rating);
        let streak = self
            .record_review(&mut tx, request, &applied, xp_earned)
            .await?;

        tx.commit().await.map_err(classify)?;

        let total_xp: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(amount), 0) FROM xp_events WHERE user_id = ?1")
                .bind(&user_id)
                .fetch_one(self.pool())
                .await
                .map_err(classify)?;

        Ok(ReviewOutcome {
            card_id: request.card,
            memory_state: applied.memory_state,
            preview: applied.preview,
            streak,
            xp_earned,
            level: level_of(total_xp),
            conflict_retried: conflict_count > 0,
        })
    }

    /// 한 번의 적용 시도. 충돌하면 `Conflict`를 돌려줄 뿐 트랜잭션은 유지한다.
    pub(crate) async fn try_apply(
        &self,
        tx: &mut Tx,
        request: &ReviewRequest,
    ) -> StoreResult<ReviewAttempt> {
        let card_id = request.card.to_string();
        let user_id = request.user.to_string();

        // 소유권을 함께 확인한다. 이 Card가 내 것이 아니면 `NotFound`로 보낸다 —
        // 존재 여부를 새면 다른 사용자의 Card를 추측할 수 있다.
        let row: Option<StateRow> = sqlx::query_as::<_, StateRow>(
            "SELECT cs.card_id, cs.state, cs.stability, cs.difficulty,
                        cs.elapsed_days, cs.scheduled_days, cs.due_at,
                        cs.last_review_at, cs.introduced_at, cs.reps, cs.lapses, cs.rev
                 FROM card_states cs
                 JOIN cards c ON c.id = cs.card_id
                 WHERE cs.card_id = ?1
                   AND c.user_id = ?2
                   AND c.deleted_at IS NULL",
        )
        .bind(&card_id)
        .bind(&user_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify)?;

        let Some(row) = row else {
            return Err(StoreError::NotFound);
        };
        let current = row.to_memory_state()?;
        let preset = self.load_preset(tx, &user_id).await?;

        let memory_state = self
            .scheduler()
            .apply(&current, request.rating, preset, request.reviewed_at)
            .map_err(|_| StoreError::Invalid("스케줄러가 계산을 거부했다"))?;
        let preview = self
            .scheduler()
            .preview(&current, preset, request.reviewed_at)
            .map_err(|_| StoreError::Invalid("스케줄러가 계산을 거부했다"))?;

        // `introduced_at` 은 첫 복습에서만 채운다. `new_per_day` 제한이 "오늘 몇 개를
        // **처음** 봤는가"를 세야 하는데 `last_review_at` 으로는 그릴 수 없다 —
        // 오래된 단어를 오늘 다시 보면 갱신되기 때문이다.
        let updated = sqlx::query(
            "UPDATE card_states
             SET state = ?2, stability = ?3, difficulty = ?4,
                 elapsed_days = ?5, scheduled_days = ?6, due_at = ?7,
                 last_review_at = ?8, introduced_at = COALESCE(introduced_at, ?8),
                 reps = ?9, lapses = ?10,
                 rev = rev + 1, updated_at = ?11
             WHERE card_id = ?1 AND rev = ?12",
        )
        .bind(&card_id)
        .bind(state_name(memory_state.state))
        .bind(memory_state.memory.map(|m| m.stability))
        .bind(memory_state.memory.map(|m| m.difficulty))
        .bind(memory_state.elapsed_days)
        .bind(memory_state.scheduled_days)
        .bind(unix_seconds(memory_state.due_at))
        .bind(memory_state.last_review_at.map(unix_seconds))
        .bind(i64::from(memory_state.reps))
        .bind(i64::from(memory_state.lapses))
        .bind(now_unix_seconds())
        .bind(row.rev)
        .execute(&mut **tx)
        .await
        .map_err(classify)?;

        if updated.rows_affected() == 0 {
            return Ok(ReviewAttempt::Conflict);
        }

        let (prev_stability, next_stability) = match (current.memory, memory_state.memory) {
            (Some(a), Some(b)) => (Some(a.stability), Some(b.stability)),
            _ => (None, None),
        };

        Ok(ReviewAttempt::Applied(Box::new(AppliedReview {
            memory_state,
            preview,
            prev_stability,
            next_stability,
        })))
    }

    /// `review_log`와 `streaks`, `xp_events`를 갱신하고 갱신된 Streak을 돌려준다.
    async fn record_review(
        &self,
        tx: &mut Tx,
        request: &ReviewRequest,
        applied: &AppliedReview,
        xp_earned: u32,
    ) -> StoreResult<Streak> {
        let user_id = request.user.to_string();
        let now = now_unix_seconds();
        let review_id = review_id_for(request);

        sqlx::query(
            "INSERT INTO review_log
                 (id, user_id, card_id, rating, reviewed_at, local_date,
                  duration_ms, prev_stability, next_stability, rev, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10)",
        )
        .bind(&review_id)
        .bind(&user_id)
        .bind(request.card.to_string())
        .bind(i64::from(request.rating.as_u8()))
        .bind(unix_seconds(request.reviewed_at))
        .bind(request.local_date.to_string())
        .bind(request.duration_ms.map(i64::from))
        .bind(applied.prev_stability)
        .bind(applied.next_stability)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(classify)?;

        let existing: Option<StreakRow> = sqlx::query_as::<_, StreakRow>(
            "SELECT current_count, longest_count, last_review_date
                 FROM streaks WHERE user_id = ?1",
        )
        .bind(&user_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify)?;

        let mut streak = existing
            .as_ref()
            .map_or_else(Streak::default, StreakRow::to_streak);
        streak.apply_review(request.local_date);

        sqlx::query(
            "INSERT INTO streaks
                 (user_id, current_count, longest_count, last_review_date, rev, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(user_id) DO UPDATE SET
                 current_count = excluded.current_count,
                 longest_count = excluded.longest_count,
                 last_review_date = excluded.last_review_date,
                 rev = streaks.rev + 1,
                 updated_at = excluded.updated_at",
        )
        .bind(&user_id)
        .bind(i64::from(streak.current))
        .bind(i64::from(streak.longest))
        .bind(streak.last_review_date.map(|d| d.to_string()))
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(classify)?;

        if xp_earned > 0 {
            sqlx::query(
                "INSERT INTO xp_events (id, user_id, kind, amount, source_id, occurred_at)
                 VALUES (?1, ?2, 'review', ?3, ?4, ?5)",
            )
            .bind(format!("{review_id}-xp"))
            .bind(&user_id)
            .bind(i64::from(xp_earned))
            .bind(&review_id)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(classify)?;
        }

        Ok(streak)
    }

    /// 사용자의 Retention Preset. 모르는 값이면 기준값으로 떨어뜨린다.
    ///
    /// `NULL`이어도 `Balanced`로 간다. 프리셋 열이 없을 수 있는 시나리오
    /// (사용자별 파라미터 학습을 붙이면서 열을 다시 쓰는 중)를 고려한 방어다.
    pub(crate) async fn load_preset(
        &self,
        tx: &mut Tx,
        user_id: &str,
    ) -> StoreResult<RetentionPreset> {
        let raw: Option<String> = sqlx::query_scalar("SELECT retention FROM users WHERE id = ?1")
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(classify)?;

        Ok(match raw.as_deref() {
            Some("diligent") => RetentionPreset::Diligent,
            Some("frugal") => RetentionPreset::Frugal,
            _ => RetentionPreset::Balanced,
        })
    }
}

fn level_of(total_xp: i64) -> LevelProgress {
    xp_progress(u64::try_from(total_xp).unwrap_or(0))
}

fn state_name(state: ReviewState) -> &'static str {
    match state {
        ReviewState::New => "new",
        ReviewState::Review => "review",
    }
}

/// `review_log.id`는 uuid가 아니라 결정적 값으로 만든다.
///
/// 같은 `(card, reviewed_at, rating, user)`에 대한 재요청이 같은 행에 닿게 된다.
/// 네트워크 재시도로 Review가 두 번 쌓이는 것을 막는다 — Streak과 XP가
/// 두 배로 오르는 것보다 한 번이 낫다.
fn review_id_for(request: &ReviewRequest) -> String {
    format!(
        "{}-{}-{}-{}",
        request.card,
        request.reviewed_at.unix_timestamp(),
        request.rating.as_u8(),
        request.user
    )
}

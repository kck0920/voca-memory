use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use voca_domain::{LevelProgress, MemoryState, Preview, Rating, Streak};

use crate::error::{StoreResult, invalid};
use crate::{CardId, DeckId, SenseView, UserId, WordView};

/// 낙관적 동시성 재시도 상한.
///
/// 한 Review를 반영할 때 다른 기기의 쓰기와 겹치면 최신 상태를 다시 읽고
/// 스케줄을 재계산한다. 이 횟수만큼 실패하면 포기하고 사용자에게 재시도를
/// 요청한다. 같은 Card를 수십 회 동시 복습해야 도달하므로 상한을 넉넉히 둔다.
pub const MAX_CONFLICT_RETRIES: u32 = 8;

/// 어떤 Card를 몇 개 풀지.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudyRequest {
    pub user: UserId,
    /// `None`이면 사용자의 모든 덱.
    pub deck: Option<DeckId>,
    pub now: OffsetDateTime,
    pub limit: u32,
}

impl StudyRequest {
    pub fn new(user: UserId, now: OffsetDateTime, limit: u32) -> Self {
        Self {
            user,
            deck: None,
            now,
            limit,
        }
    }

    pub fn in_deck(mut self, deck: DeckId) -> Self {
        self.deck = Some(deck);
        self
    }
}

/// 풀 Card 하나.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueuedCard {
    pub card: DeckViewCardRef,
    pub word: WordView,
    pub sense: SenseView,
    pub memory_state: MemoryState,
    /// 정렬에 쓴 현재 기억 회수 가능성(0~1). 낮을수록 먼저 나온다.
    ///
    /// `MemoryState::new`인 Card는 계산할 대상이 없어 `1.0`을 넣는다 — 그런 Card는
    /// 신규로 분류돼 별도 순서를 탄다.
    pub retrievability: f32,
}

/// 큐에 든 Card의 신원. 전체 덱이 아니라 그 Card가 속한 덱만 가리킨다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckViewCardRef {
    pub card_id: CardId,
    pub deck_id: DeckId,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StudyQueue {
    pub cards: Vec<QueuedCard>,
    /// `limit` 때문에 밀려난 복습 Card 수. "앞으로 N개 더 있다"를 보여줄 때 쓴다.
    pub reviews_remaining: u32,
    /// 오늘의 신규 한도 중 아직 안 쓴 몫. 0이면 신규가 더 안 나온다.
    pub new_remaining_today: u32,
}

impl StudyQueue {
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 오늘 더 풀 수 있는 복습 Card가 남아 있는가.
    pub fn has_more(&self) -> bool {
        self.reviews_remaining > 0
    }
}

/// Review 제출.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewRequest {
    pub user: UserId,
    pub card: CardId,
    pub rating: Rating,
    pub reviewed_at: OffsetDateTime,
    /// 세션 동안 고정된 오프셋으로 환산한 현지 날짜. Streak 판정 기준.
    pub local_date: Date,
    /// 오프셋이 이 범위를 벗어나면 `Streak` 계산에 쓰지 않고 UTC로 떨어뜨린다.
    pub utc_offset_seconds: i32,
    pub duration_ms: Option<u32>,
}

impl ReviewRequest {
    pub fn new(
        user: UserId,
        card: CardId,
        rating: Rating,
        reviewed_at: OffsetDateTime,
        local_date: Date,
        utc_offset_seconds: i32,
    ) -> Self {
        Self {
            user,
            card,
            rating,
            reviewed_at,
            local_date,
            utc_offset_seconds,
            duration_ms: None,
        }
    }

    pub fn with_duration(mut self, ms: u32) -> Self {
        self.duration_ms = Some(ms);
        self
    }

    /// `local_date`가 `reviewed_at`와 모순되지 않는지 본다.
    ///
    /// 오프셋이 ±14시간으로 잘리므로, 올바른 계산이면 두 날짜는 최대 하루 차이다.
    /// 그보다 벌어졌다는 것은 시계 조작이거나 계산 버그다 — 어느 쪽이든 Streak
    /// 계산에 쓰면 안 된다.
    pub fn validate(&self) -> StoreResult<()> {
        let utc_date = self.reviewed_at.date();
        let drift_days = (self.local_date - utc_date).whole_days().abs();
        if drift_days > 1 {
            return Err(invalid("local_date가 reviewed_at에서 하루 이상 어긋난다"));
        }
        Ok(())
    }
}

/// Review 반영 결과.
///
/// 다음 복습 시각·4개 Rating의 도착 지점·Streak·XP·Level을 **한 번에** 돌려준다.
/// 호출부가 두 번째 조회를 해야 하면 사용자에게 빈 화면이 보인다.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewOutcome {
    pub card_id: CardId,
    /// 반영 후의 스케줄러 상태. `due_at`이 다음 복습 시각이다.
    pub memory_state: MemoryState,
    /// 4개 Rating이 각각 어디로 보내는지. Study 화면의 버튼 라벨용.
    pub preview: Preview,
    pub streak: Streak,
    pub xp_earned: u32,
    pub level: LevelProgress,
    /// 낙관적 동시성 충돌이 났는지. 감사용 — 사용자에게는 노출하지 않는다.
    pub conflict_retried: bool,
}

impl ReviewOutcome {
    /// 이 Review가 이번 세션에서 다시 보여줘야 하는 Card인가.
    ///
    /// 순수 FSRS-6은 learning step 대신 짧은 interval로 이 신호를 낸다.
    /// ([`docs/adr/0009`](../../docs/adr/0009-no-learning-steps-pure-fsrs-6.md))
    pub fn requeues_this_session(&self) -> bool {
        self.memory_state.is_intra_session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};
    use voca_domain::Id;

    fn request() -> ReviewRequest {
        ReviewRequest::new(
            Id::parse("0195f0a0-0000-7000-8000-000000000001").unwrap(),
            Id::parse("0195f0a0-0000-7000-8000-000000000002").unwrap(),
            Rating::Good,
            datetime!(2026-03-02 09:00:00 UTC),
            date!(2026 - 03 - 02),
            9 * 3600,
        )
    }

    #[test]
    fn request_accepts_a_sane_local_date() {
        assert!(request().validate().is_ok());
    }

    #[test]
    fn request_rejects_a_local_date_that_contradicts_the_timestamp() {
        // 09:00 UTC에 클라이언트가 1969년을 보냈다. 오프셋으로는 그날이 되지 않는다.
        let mut r = request();
        r.local_date = date!(1969 - 01 - 01);
        assert!(matches!(r.validate(), Err(crate::StoreError::Invalid(_))));
    }

    #[test]
    fn request_accepts_a_local_date_on_the_other_side_of_the_date_line() {
        // 22:00 UTC는 KST로는 다음날 07:00이다. 하루 차이는 정상이다.
        let mut r = request();
        r.reviewed_at = datetime!(2026-03-02 22:00:00 UTC);
        r.local_date = date!(2026 - 03 - 03);
        assert!(r.validate().is_ok());
    }

    #[test]
    fn duration_is_optional_and_settable() {
        assert_eq!(request().duration_ms, None);
        assert_eq!(request().with_duration(1_500).duration_ms, Some(1_500));
    }

    #[test]
    fn empty_queue_has_nothing_more() {
        let q = StudyQueue {
            cards: Vec::new(),
            reviews_remaining: 0,
            new_remaining_today: 0,
        };
        assert!(q.is_empty());
        assert!(!q.has_more());
    }

    #[test]
    fn remaining_reviews_signal_that_more_work_is_waiting() {
        let q = StudyQueue {
            cards: Vec::new(),
            reviews_remaining: 12,
            new_remaining_today: 0,
        };
        assert!(q.is_empty());
        assert!(
            q.has_more(),
            "밀려난 복습이 있으면 큐가 비어 보여도 계속할 수 있다"
        );
    }

    #[test]
    fn study_request_defaults_to_every_deck() {
        let r = StudyRequest::new(
            Id::parse("0195f0a0-0000-7000-8000-000000000001").unwrap(),
            datetime!(2026-03-02 09:00:00 UTC),
            20,
        );
        assert_eq!(r.deck, None);
        assert_eq!(r.limit, 20);
    }

    #[test]
    fn conflict_budget_is_wide_enough_to_be_practical() {
        // 같은 Card를 이 횟수만큼 동시에 복습하는 일은 현실에 없다. 그만큼 넉넉해야
        // 충돌로 인한 재전송이 사실상 발생하지 않는다.
        let budget = MAX_CONFLICT_RETRIES;
        assert!(budget >= 4, "재시도 예산이 {budget}이면 너무 빠듯하다");
    }
}

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::Id;

/// Card의 명명된 묶음. 하나의 학습 목표를 나타낸다.
///
/// Card는 정확히 하나의 Deck에 속한다. 여러 덱에 넣고 싶으면 Card를 복제한다 —
/// [`Card::cloned_from`]이 그 출처를 기록한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deck {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    /// 하루 목표 개수. `None`이면 무제한.
    pub daily_goal: Option<u32>,
}

/// 학습자가 복습하는 단위. 정확히 하나의 Sense를 향한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: Id,
    pub deck_id: Id,
    pub sense_id: Id,
    /// 이 Card를 복제한 원본. 새로 만든 Card는 `None`.
    pub cloned_from: Option<Id>,
}

impl Card {
    /// 복습 이력이 없는 새 Card. `cloned_from`이 없다 — 복제본은 별도로 만든다.
    pub fn new(id: Id, deck_id: Id, sense_id: Id) -> Self {
        Self {
            id,
            deck_id,
            sense_id,
            cloned_from: None,
        }
    }
}

/// 학습자가 Card를 본 직후 매기는 4단계 평가.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    pub const ALL: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];

    /// 저장되는 값. `review_log.rating`과 1:1로 대응한다.
    pub fn as_u8(self) -> u8 {
        match self {
            Rating::Again => 1,
            Rating::Hard => 2,
            Rating::Good => 3,
            Rating::Easy => 4,
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Rating::Again),
            2 => Some(Rating::Hard),
            3 => Some(Rating::Good),
            4 => Some(Rating::Easy),
            _ => None,
        }
    }
}

/// Card가 스케줄러 입장에서 처럼 있는 단계.
///
/// **이 열거형이 두 값뿐인 것은 의도적이다.** Anki의 `new | learning | review |
/// relearning` 4단계를 따르지 않는다. 우리는 순수 FSRS-6을 쓰는데, FSRS-6은
/// learning step을 모델링하지 않는다 — 망각하면 Stability가 떨어지고 interval이
/// 수분으로 짧아질 뿐, 별도의 step 기계가 없다.
///
/// `Again` 직후 같은 세션에서 다시 보여주는 동작은 여기서 나오지 않는다.
/// 그건 [`MemoryState::is_intra_session`]이 읽는 `interval < 1일` 조건이 만든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// 한 번도 복습하지 않은 Card. FSRS는 이 상태에서 Stability·Difficulty를 0으로 본다.
    New,
    /// 한 번 이상 복습한 Card.
    Review,
}

/// 스케줄러가 관리하는 Card의 핵심 스칼라 두 개.
///
/// Card 본문과 분리되어 Card와 1:1로 대응한다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// 기억이 유지될 것으로 보이는 기간(일 단위).
    pub stability: f32,
    /// 다시 떠올리는 데 드는 어려움. 클수록 Stability 증가 배율이 낮아진다.
    pub difficulty: f32,
}

/// Card와 1:1로 대응하는 스케줄러 상태.
///
/// 저장되지 않는 파생 지표(Mastery 등)는 여기서 계산한다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MemoryState {
    pub state: ReviewState,
    /// `None`이면 아직 Stability·Difficulty가 없다(`ReviewState::New`와 일관).
    pub memory: Option<Memory>,
    pub elapsed_days: f32,
    pub scheduled_days: f32,
    pub due_at: OffsetDateTime,
    pub last_review_at: Option<OffsetDateTime>,
    pub reps: u32,
    /// 망각한 횟수. `Rating::Again`을 누른 횟수.
    pub lapses: u32,
}

impl MemoryState {
    /// 아직 복습하지 않은 Card의 초기 상태.
    pub fn new(due_at: OffsetDateTime) -> Self {
        Self {
            state: ReviewState::New,
            memory: None,
            elapsed_days: 0.0,
            scheduled_days: 0.0,
            due_at,
            last_review_at: None,
            reps: 0,
            lapses: 0,
        }
    }

    /// 복습 대상인가. 저장된 플래그가 아니라 시각 비교의 결과다.
    pub fn is_due(&self, now: OffsetDateTime) -> bool {
        now >= self.due_at
    }

    /// 이번 세션 안에서 다시 보여줘야 하는가.
    ///
    /// interval이 1일 미만이면 그 Card는 그날 안에서 여러 번 훑어야 한다.
    /// 순수 FSRS-6은 learning step 대신 짧은 interval로 이 동작을 만든다.
    pub fn is_intra_session(&self) -> bool {
        self.scheduled_days < 1.0
    }
}

/// Card를 제시하고 Rating을 받는 한 번의 상호작용.
///
/// Streak과 XP의 집계 단위다. 세션 목표 달성 여부는 여기에 관여하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub card_id: Id,
    pub rating: Rating,
    pub reviewed_at: OffsetDateTime,
    /// 세션 동안 고정된 오프셋으로 환산한 현지 날짜. Streak 판정의 기준.
    pub local_date: Date,
    /// 응답까지 걸린 시간(밀리초). 선택 사항.
    pub duration_ms: Option<u32>,
}

/// 보너스를 계산할 수 있도록 한 세션에 모인 Review 묶음.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionSummary {
    pub review_count: u32,
    pub again_count: u32,
}

impl SessionSummary {
    pub fn record(&mut self, rating: Rating) {
        self.review_count = self.review_count.saturating_add(1);
        if rating == Rating::Again {
            self.again_count = self.again_count.saturating_add(1);
        }
    }

    /// 세션 목표를 채웠는가.
    ///
    /// Streak 판정에는 쓰지 않는다([`CONTEXT.md`](../../CONTEXT.md)의 Session 정의).
    /// 대시보드 표시용이다.
    pub fn meets_goal(&self, goal: Option<u32>) -> bool {
        match goal {
            None => true,
            Some(goal) => self.review_count >= goal,
        }
    }
}

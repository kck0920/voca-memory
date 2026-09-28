use serde::{Deserialize, Serialize};
use voca_domain::Rating;

use crate::{CardId, DeckId, SenseId, UserId, Versioned, WordId};

/// 동기화 한 번에 받을 범위.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRequest {
    pub user: UserId,
    /// 이 revision 이후의 변경만. 클라이언트가 마지막으로 본 지점.
    pub since: crate::Revision,
    /// 한 번에 받을 양의 상한. 넘으면 같은 `since`로 다시 부른다.
    pub limit: u32,
}

impl ChangeRequest {
    pub const DEFAULT_LIMIT: u32 = 500;

    pub fn new(user: UserId, since: crate::Revision) -> Self {
        Self {
            user,
            since,
            limit: Self::DEFAULT_LIMIT,
        }
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChangePage {
    pub changes: Vec<Change>,
    /// 이 페이지를 다 적용한 뒤 클라이언트가 저장할 revision.
    ///
    /// `changes`가 비었어도 값은 갱신된다 — 그 사이에 아무것도 안 바뀌었다는
    /// 사실도 갱신이다.
    pub watermark: crate::Revision,
    /// 다음 페이지가 있는가. 있으면 같은 `since`로 다시 불러온다.
    pub has_more: bool,
}

impl ChangePage {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// 바뀐 것 하나.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Card(Versioned<CardRow>),
    Deck(Versioned<DeckRow>),
    Sense(Versioned<SenseRow>),
    Word(Versioned<WordRow>),
    /// soft delete된 Card. 본문 없이 사라졌다는 사실만 전한다.
    CardTombstone {
        id: CardId,
        revision: crate::Revision,
    },
    /// soft delete된 Deck.
    DeckTombstone {
        id: DeckId,
        revision: crate::Revision,
    },
    /// 숨겨진 Sense.
    SenseTombstone {
        id: SenseId,
        revision: crate::Revision,
    },
}

impl Change {
    pub fn revision(&self) -> crate::Revision {
        match self {
            Change::Card(v) => v.revision,
            Change::Deck(v) => v.revision,
            Change::Sense(v) => v.revision,
            Change::Word(v) => v.revision,
            Change::CardTombstone { revision, .. }
            | Change::DeckTombstone { revision, .. }
            | Change::SenseTombstone { revision, .. } => *revision,
        }
    }

    pub fn is_tombstone(&self) -> bool {
        matches!(
            self,
            Change::CardTombstone { .. }
                | Change::DeckTombstone { .. }
                | Change::SenseTombstone { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardRow {
    pub id: CardId,
    pub deck_id: DeckId,
    pub sense_id: SenseId,
    pub cloned_from: Option<CardId>,
    /// 이 Card의 스케줄러 상태. **Card 본문과 반드시 함께 동기화된다** — 이게
    /// 빠지면 기기를 바꿨을 때 복습 일정 전체가 뒤집힌다.
    pub state: StateRow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateRow {
    pub state: String,
    pub stability: Option<f32>,
    pub difficulty: Option<f32>,
    pub elapsed_days: f32,
    pub scheduled_days: f32,
    pub due_at: i64,
    pub last_review_at: Option<i64>,
    pub reps: u32,
    pub lapses: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckRow {
    pub id: DeckId,
    pub name: String,
    pub description: Option<String>,
    pub daily_goal: u32,
    pub new_per_day: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SenseRow {
    pub id: SenseId,
    pub word_id: WordId,
    pub kind: String,
    pub source: String,
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordRow {
    pub id: WordId,
    pub lemma: String,
    pub source: String,
    pub phonetic: Option<String>,
    pub audio_url: Option<String>,
}

/// Review 기록은 append-only라 변경 목록에 없다. 클라이언트는 Streak·XP를
/// `review_log`에서 다시 계산하지 않고 서버가 준 파생값을 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub count: u32,
    pub last_review_at: Option<i64>,
    pub last_rating: Option<Rating>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(revision: u64) -> Change {
        Change::CardTombstone {
            id: CardId::from([1u8; 16]),
            revision: revision.into(),
        }
    }

    #[test]
    fn every_change_reports_its_revision() {
        assert_eq!(change(5).revision(), crate::Revision::from(5));
        assert_eq!(change(5).revision().as_u64(), 5);
    }

    #[test]
    fn tombstones_are_tombstones() {
        assert!(change(1).is_tombstone());
        assert!(
            !Change::Deck(Versioned::new(
                DeckRow {
                    id: DeckId::from([0u8; 16]),
                    name: "x".into(),
                    description: None,
                    daily_goal: 20,
                    new_per_day: 10,
                },
                crate::Revision::from(1)
            ))
            .is_tombstone()
        );
    }

    #[test]
    fn watermark_advances_even_with_no_changes() {
        let page = ChangePage {
            changes: vec![],
            watermark: crate::Revision::from(42),
            has_more: false,
        };
        assert!(page.is_empty());
        assert_eq!(
            page.watermark.as_u64(),
            42,
            "변경이 없더라도 watermark를 올려야 클라이언트가 재요청을 멈춘다"
        );
    }

    #[test]
    fn has_more_means_call_again_with_the_same_since() {
        let page = ChangePage {
            changes: vec![change(1)],
            watermark: crate::Revision::from(1),
            has_more: true,
        };
        assert!(page.has_more);
    }
}

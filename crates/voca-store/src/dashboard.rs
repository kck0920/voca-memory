use serde::Serialize;
use time::{Date, OffsetDateTime};
use voca_domain::{LevelProgress, Streak};

use crate::DeckId;

/// 대시보드가 한 번에 필요로 하는 것.
///
/// 패널별로 따로 조회하게 하면 숫자가 서로 어긋난다 — 이 화면의 핵심 Streak 숫자가
/// Streak, Review, 진도 세 번의 조회 결과로 나뉘어 있으면 그 사이에 Review가 끼어든다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dashboard {
    /// 조회 기준 시각. 이후 Review는 이 화면에 반영되지 않는다.
    pub as_of: OffsetDateTime,
    /// 이 시각의 현지 날짜.
    pub local_date: Date,
    pub streak: Streak,
    pub level: LevelProgress,
    /// 오늘 아직 풀지 않은 복습 Card 수.
    pub reviews_due: u32,
    /// 오늘 아직 배우지 않은 신규 Card 수 (일일 한도에서 남은 몫 포함).
    pub new_remaining: u32,
    pub decks: Vec<DeckProgress>,
}

impl Dashboard {
    /// 오늘 풀 것이 남아 있는가. Streak는 오늘 Review가 없어도 유지된다 — 이
    /// 판정과 별개다.
    pub fn has_work_today(&self) -> bool {
        self.reviews_due > 0 || self.new_remaining > 0
    }

    /// 오늘의 Review가 하나도 없었다. 이때 Streak 숫자는 깨지지만 끊기지는 않는다.
    pub fn is_streak_at_risk(&self) -> bool {
        self.has_work_today() && self.streak.last_review_date != Some(self.local_date)
    }

    pub fn deck(&self, id: DeckId) -> Option<&DeckProgress> {
        self.decks.iter().find(|d| d.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DeckProgress {
    pub id: DeckId,
    /// 이 덱에 배정된 Card 수. `deleted_at`이 찍히지 않은 것만.
    pub total: u32,
    /// 한 번 이상 복습한 Card 수.
    pub seen: u32,
    /// 지금 Due인 Card 수.
    pub due: u32,
    /// `New` 상태인 Card 수.
    pub fresh: u32,
}

impl DeckProgress {
    /// 진도 비율(0~100).
    pub fn progress_percent(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.seen as u64 * 100 / self.total as u64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn progress(total: u32, seen: u32) -> DeckProgress {
        DeckProgress {
            id: DeckId::from([0u8; 16]),
            total,
            seen,
            due: 0,
            fresh: total.saturating_sub(seen),
        }
    }

    #[test]
    fn progress_is_a_percentage_of_seen_over_total() {
        assert_eq!(progress(200, 50).progress_percent(), 25);
        assert_eq!(progress(200, 200).progress_percent(), 100);
        assert_eq!(progress(200, 0).progress_percent(), 0);
    }

    #[test]
    fn an_empty_deck_is_zero_percent_not_a_division_error() {
        assert_eq!(progress(0, 0).progress_percent(), 0);
    }

    fn dashboard(reviews_due: u32, last: Option<Date>) -> Dashboard {
        Dashboard {
            as_of: datetime!(2026-03-02 09:00:00 UTC),
            local_date: date!(2026 - 03 - 02),
            streak: Streak {
                current: 7,
                longest: 12,
                last_review_date: last,
            },
            level: LevelProgress {
                level: 3,
                xp_into_level: 40,
                xp_span: 1200,
            },
            reviews_due,
            new_remaining: 0,
            decks: vec![progress(200, 50)],
        }
    }

    #[test]
    fn work_today_is_independent_of_the_streak() {
        // 복습할 게 없는데 Streak 숫자가 남아 있다. 끊긴 게 아니다.
        let d = dashboard(0, Some(date!(2026 - 03 - 01)));
        assert!(!d.has_work_today());
        assert_eq!(d.streak.current, 7);
        assert!(!d.is_streak_at_risk());
    }

    #[test]
    fn a_streak_is_at_risk_only_when_there_is_still_work_today() {
        let d = dashboard(12, Some(date!(2026 - 03 - 01)));
        assert!(
            d.is_streak_at_risk(),
            "할 게 있는데 오늘 아직 안 했다면 위험"
        );

        let d = dashboard(12, Some(date!(2026 - 03 - 02)));
        assert!(!d.is_streak_at_risk(), "오늘 이미 했다");
    }

    #[test]
    fn deck_lookup_finds_the_named_deck() {
        let d = dashboard(0, None);
        let id = d.decks[0].id;
        assert!(d.deck(id).is_some());
        assert!(d.deck(DeckId::from([9u8; 16])).is_none());
    }
}

use serde::{Deserialize, Serialize};
use time::{Date, Duration};

/// 하루에 Review가 1건 이상 있었던 날들이 이어진 구간.
///
/// Rating이 무엇이든 이어진다 — `Again`만 눌러도 그날은 Streak에 들어간다.
/// 보상은 복습 행위이지 정답이 아니기 때문이다([`gamify`](crate::gamify)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Streak {
    pub current: u32,
    pub longest: u32,
    /// 마지막으로 Review가 있었던 현지 날짜. Streak 판정에서 날짜를 되돌아가지 못하게 한다.
    pub last_review_date: Option<Date>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreakUpdate {
    /// 첫 Review.
    Started,
    /// 같은 날의 추가 Review, 또는 과거 날짜의 Review. 변화 없음.
    Unchanged,
    /// 전 날짜에 이어 Review. `current`가 1 늘었다.
    Extended,
    /// 공백이 있어 다시 시작. `current`가 1로 돌아갔다.
    Restarted,
}

impl Streak {
    /// Review가 하나 쌓였을 때의 결과.
    pub fn apply_review(&mut self, local_date: Date) -> StreakUpdate {
        let update = match self.last_review_date {
            None => StreakUpdate::Started,
            Some(last) if local_date <= last => StreakUpdate::Unchanged,
            Some(last) if days_between(last, local_date) == 1 => StreakUpdate::Extended,
            Some(_) => StreakUpdate::Restarted,
        };

        match update {
            StreakUpdate::Started | StreakUpdate::Restarted => {
                self.current = 1;
                self.last_review_date = Some(local_date);
            }
            StreakUpdate::Extended => {
                self.current = self.current.saturating_add(1);
                self.last_review_date = Some(local_date);
            }
            StreakUpdate::Unchanged => {}
        }

        self.longest = self.longest.max(self.current);
        update
    }
}

/// `to - from`을 일 단위 정수로. 음수면 0으로 본다.
fn days_between(from: Date, to: Date) -> i64 {
    let delta: Duration = to - from;
    delta.whole_days().max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn first_review_starts_at_one() {
        let mut streak = Streak::default();
        assert_eq!(
            streak.apply_review(date!(2026 - 03 - 02)),
            StreakUpdate::Started
        );
        assert_eq!(streak.current, 1);
        assert_eq!(streak.longest, 1);
    }

    #[test]
    fn many_reviews_in_one_day_count_as_one_day() {
        let mut streak = Streak::default();
        for _ in 0..20 {
            streak.apply_review(date!(2026 - 03 - 02));
        }
        assert_eq!(streak.current, 1);
        assert_eq!(streak.longest, 1);
    }

    #[test]
    fn consecutive_days_extend() {
        let mut streak = Streak::default();
        let mut expected = 0;
        for day in 2..=10 {
            streak.apply_review(date!(2026 - 03 - 01) + Duration::days(day));
            expected += 1;
            assert_eq!(streak.current, expected, "day {day}");
        }
    }

    #[test]
    fn a_gap_restarts_but_keeps_the_record() {
        let mut streak = Streak::default();
        for day in 2..=6 {
            streak.apply_review(date!(2026 - 03 - 01) + Duration::days(day));
        }
        assert_eq!(streak.current, 5);

        // 3일 공백
        streak.apply_review(date!(2026 - 03 - 10));
        assert_eq!(streak.current, 1);
        assert_eq!(streak.longest, 5, "the record must survive a gap");
    }

    #[test]
    fn going_back_in_time_does_not_punish_or_reward() {
        let mut streak = Streak::default();
        streak.apply_review(date!(2026 - 03 - 10));
        // 시계 오작동이나 날짜변경선 이동으로 과거 날짜가 들어올 수 있다.
        assert_eq!(
            streak.apply_review(date!(2026 - 03 - 08)),
            StreakUpdate::Unchanged
        );
        assert_eq!(streak.current, 1);
        assert_eq!(
            streak.last_review_date,
            Some(date!(2026 - 03 - 10)),
            "a stale date must not overwrite the boundary"
        );
    }

    #[test]
    fn only_again_still_counts_as_a_day() {
        // Rating은 Streak 계산에 들어가지 않는다. 도메인 타입 자체가 Date만 받는다.
        let mut streak = Streak::default();
        streak.apply_review(date!(2026 - 03 - 02));
        streak.apply_review(date!(2026 - 03 - 03));
        assert_eq!(streak.current, 2);
    }

    #[test]
    fn month_and_year_rollover_keeps_the_chain() {
        let mut streak = Streak::default();
        streak.apply_review(date!(2026 - 12 - 31));
        assert_eq!(
            streak.apply_review(date!(2027 - 01 - 01)),
            StreakUpdate::Extended
        );
        assert_eq!(streak.current, 2);
    }

    #[test]
    fn leap_day_does_not_break_the_chain() {
        let mut streak = Streak::default();
        streak.apply_review(date!(2028 - 02 - 28));
        assert_eq!(
            streak.apply_review(date!(2028 - 02 - 29)),
            StreakUpdate::Extended
        );
        assert_eq!(
            streak.apply_review(date!(2028 - 03 - 01)),
            StreakUpdate::Extended
        );
        assert_eq!(streak.current, 3);
    }

    #[test]
    fn saturates_instead_of_overflowing() {
        let mut streak = Streak {
            current: u32::MAX,
            longest: u32::MAX,
            last_review_date: Some(date!(2026 - 03 - 02)),
        };
        assert_eq!(
            streak.apply_review(date!(2026 - 03 - 03)),
            StreakUpdate::Extended
        );
        assert_eq!(streak.current, u32::MAX);
    }
}

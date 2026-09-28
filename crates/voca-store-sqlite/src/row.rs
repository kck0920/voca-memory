use sqlx::FromRow;
use time::format_description::well_known;
use time::{Date, OffsetDateTime};
use voca_domain::{Memory, MemoryState, ReviewState, Streak};
use voca_store::StoreError;

/// `users` 행.
#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub timezone: String,
    /// `diligent | balanced | frugal`
    pub retention: String,
    pub created_at: i64,
}

/// `decks` 행. soft delete는 이 어댑터가 걸러낸 뒤에야 이 타입이 된다.
#[derive(Debug, Clone, FromRow)]
pub struct DeckRow {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub daily_goal: i64,
    pub new_per_day: i64,
    pub rev: i64,
}

/// `cards` 행.
#[derive(Debug, Clone, FromRow)]
pub struct CardRow {
    pub id: String,
    pub user_id: String,
    pub deck_id: String,
    pub sense_id: String,
    pub cloned_from: Option<String>,
    pub rev: i64,
}

/// `card_states` 행.
///
/// `stability`와 `difficulty`가 nullable인 것은 FSRS가 "아직 학습 전"을 Stability 0으로
/// 표현하지 않기 때문이다 — 그 상태는 `state = 'new'`이고 두 값이 아예 없다.
#[derive(Debug, Clone, FromRow)]
pub struct StateRow {
    pub card_id: String,
    pub state: String,
    // SQLite 의 REAL 은 f64 다. 도메인(`voca_domain::Memory`)은 f32 이다 — `fsrs`
    // 크레이트가 f32 를 쓰기 때문이고, 어긋나면 스케줄 값이 여기서 잘린다.
    // 어긋난 변환이 조용히 일어나지 않도록 변환 지점을 이 한 곳으로 모은다.
    pub stability: Option<f32>,
    pub difficulty: Option<f32>,
    pub elapsed_days: f32,
    pub scheduled_days: f32,
    pub due_at: i64,
    pub last_review_at: Option<i64>,
    pub introduced_at: Option<i64>,
    pub reps: i64,
    pub lapses: i64,
    pub rev: i64,
}

impl StateRow {
    /// 저장소 표현을 도메인 표현으로 바꾼다.
    ///
    /// `state = 'new'`인데 `stability`가 있으면 그건 손으로 만든 데이터다. 도메인의
    /// 불변식(`memory.is_some()` ⟺ `state != New`)을 어기는 값을 조용히 통과시키지
    /// 않는다.
    pub fn to_memory_state(&self) -> Result<MemoryState, StoreError> {
        let state = match self.state.as_str() {
            "new" => ReviewState::New,
            "review" => ReviewState::Review,
            _ => return Err(StoreError::Invalid("state 값이 알 수 없다")),
        };

        let memory = match (self.stability, self.difficulty) {
            (None, None) => None,
            (Some(stability), Some(difficulty)) => Some(Memory {
                stability,
                difficulty,
            }),
            _ => {
                return Err(StoreError::Invalid("stability와 difficulty 중 하나만 있다"));
            }
        };

        if state == ReviewState::Review && memory.is_none() {
            return Err(StoreError::Invalid("복습한 Card인데 기억 상태가 없다"));
        }
        if state == ReviewState::New && memory.is_some() {
            return Err(StoreError::Invalid("신규 Card인데 기억 상태가 있다"));
        }

        Ok(MemoryState {
            state,
            memory,
            elapsed_days: self.elapsed_days,
            scheduled_days: self.scheduled_days,
            due_at: OffsetDateTime::from_unix_timestamp(self.due_at)
                .map_err(|_| StoreError::Invalid("due_at가 범위를 벗어났다"))?,
            last_review_at: self
                .last_review_at
                .map(OffsetDateTime::from_unix_timestamp)
                .transpose()
                .map_err(|_| StoreError::Invalid("last_review_at가 범위를 벗어났다"))?,
            reps: self.reps.clamp(0, i64::from(u32::MAX)) as u32,
            lapses: self.lapses.clamp(0, i64::from(u32::MAX)) as u32,
        })
    }
}

/// `words` 행.
#[derive(Debug, Clone, FromRow)]
pub struct WordRow {
    pub id: String,
    pub lemma: String,
    pub source: String,
    pub phonetic: Option<String>,
    pub audio_url: Option<String>,
    pub rev: i64,
}

/// `senses` 행. `archived_at`은 이 어댑터가 걸러낸다.
#[derive(Debug, Clone, FromRow)]
pub struct SenseRow {
    pub id: String,
    pub word_id: String,
    pub kind: String,
    pub source: String,
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
    pub rev: i64,
}

/// `streaks` 행. review_log에서 파생한 캐시다.
#[derive(Debug, Clone, FromRow)]
pub struct StreakRow {
    pub current_count: i64,
    pub longest_count: i64,
    pub last_review_date: Option<String>,
}

impl StreakRow {
    pub fn to_streak(&self) -> Streak {
        Streak {
            current: self.current_count.clamp(0, i64::from(u32::MAX)) as u32,
            longest: self.longest_count.clamp(0, i64::from(u32::MAX)) as u32,
            // 손으로 들어간 값이라 파싱이 실패할 수 있다. 실패하면 없는 것으로
            // 보아 Streak이 하루씩 리셋되는 것보다 "이어지고 있다"가 낫다.
            last_review_date: self.last_review_date.as_deref().and_then(parse_iso_date),
        }
    }
}

/// `YYYY-MM-DD` 문자열을 `Date`로 읽는다.
///
/// `time::Date`는 `FromStr`를 구현하지 않는다 — 형식이 유연해야 하기 때문이다.
/// 우리 스키마는 형식을 하나만 쓰므로 그걸 명시한다.
pub(crate) fn parse_iso_date(raw: &str) -> Option<Date> {
    Date::parse(raw, &well_known::Iso8601::DEFAULT).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn row(state: &str, stability: Option<f32>, difficulty: Option<f32>) -> StateRow {
        StateRow {
            card_id: "c1".into(),
            state: state.into(),
            stability,
            difficulty,
            elapsed_days: 0.0,
            scheduled_days: 0.0,
            due_at: datetime!(2026-03-02 09:00:00 UTC).unix_timestamp(),
            last_review_at: None,
            introduced_at: None,
            reps: 0,
            lapses: 0,
            rev: 0,
        }
    }

    #[test]
    fn a_new_card_has_no_memory() {
        let s = row("new", None, None).to_memory_state().unwrap();
        assert_eq!(s.state, ReviewState::New);
        assert!(s.memory.is_none());
    }

    #[test]
    fn a_reviewed_card_carries_its_memory() {
        let s = row("review", Some(8.0), Some(5.0))
            .to_memory_state()
            .unwrap();
        assert_eq!(s.state, ReviewState::Review);
        assert_eq!(s.memory.unwrap().stability, 8.0);
    }

    #[test]
    fn half_a_memory_is_rejected() {
        // SQLite는 NULL을 개별적으로 허용하므로 이런 행이 생길 수 있다.
        // 그대로 통과시키면 스케줄러가 안정성 0인 것으로 계산해 Card를 망가뜨린다.
        assert!(row("review", Some(8.0), None).to_memory_state().is_err());
        assert!(row("review", None, Some(5.0)).to_memory_state().is_err());
    }

    #[test]
    fn state_and_memory_must_agree() {
        // 두 불변식이 깨진 행을 조용히 통과시키지 않는다.
        assert!(row("review", None, None).to_memory_state().is_err());
        assert!(row("new", Some(1.0), Some(1.0)).to_memory_state().is_err());
    }

    #[test]
    fn an_unknown_state_is_rejected() {
        // "relearning" 같은 값이 들어오면(구 스키마에서 옮긴 데이터) 거절한다.
        assert!(
            row("relearning", Some(1.0), Some(1.0))
                .to_memory_state()
                .is_err()
        );
    }

    #[test]
    fn a_streak_row_parses_its_date() {
        let row = StreakRow {
            current_count: 7,
            longest_count: 12,
            last_review_date: Some("2026-03-02".into()),
        };
        let s = row.to_streak();
        assert_eq!(s.current, 7);
        assert_eq!(s.longest, 12);
        assert_eq!(
            s.last_review_date,
            Some(time::macros::date!(2026 - 03 - 02))
        );
    }

    #[test]
    fn iso_dates_parse() {
        assert_eq!(
            parse_iso_date("2026-03-02"),
            Some(time::macros::date!(2026 - 03 - 02))
        );
        assert_eq!(parse_iso_date("어제쯤"), None);
        assert_eq!(parse_iso_date("2026-3-2"), None, "형식은 하나로 고정한다");
    }

    #[test]
    fn an_unparseable_streak_date_reads_as_absent() {
        // 파싱 실패를 조용히 '없음'으로 두면 Streak이 1로 리셋된다.
        // 이어지고 있는 쪽이 덜 해롭다.
        let row = StreakRow {
            current_count: 7,
            longest_count: 12,
            last_review_date: Some("어제쯤".into()),
        };
        assert_eq!(row.to_streak().last_review_date, None);
        assert_eq!(row.to_streak().current, 7, "현재 수는 보존한다");
    }

    #[test]
    fn out_of_range_timestamps_are_rejected() {
        let mut r = row("new", None, None);
        r.due_at = i64::MAX;
        assert!(r.to_memory_state().is_err());
    }
}

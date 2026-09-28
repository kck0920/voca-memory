use fsrs::{FSRS, MemoryState as FsrsMemory};
use serde::Serialize;
use time::Duration;
use time::OffsetDateTime;

use crate::{Memory, MemoryState, Rating, RetentionPreset, ReviewState};

/// 1일 미만 interval의 최단값.
///
/// `Again` 직후의 interval은 수분 단위로 나온다. 그대로 쓰면 같은 Card가 즉시
/// 다시 제시되어 무한 루프가 될 수 있다. 1분으로 묶어 최소한의 간격을 둔다.
pub const MIN_INTRA_SESSION_INTERVAL: Duration = Duration::minutes(1);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScheduleError {
    /// `fsrs` 가 계산을 거부했다. 파라미터나 상태가 유효하지 않을 때.
    Rejected,
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleError::Rejected => f.write_str("FSRS가 스케줄 계산을 거부했다"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// FSRS 스케줄러.
///
/// 결정적 함수만 노출한다 — `next_states()`는 난수나 스레드를 쓰지 않는다.
/// 같은 [`MemoryState`]와 같은 [`RetentionPreset`]를 주면 어디서 실행하든 같은 결과가 나온다.
/// (파라미터 학습 `compute_parameters`와 시뮬레이션 `simulate`는 rayon을 쓰므로
/// 이 타입에 포함하지 않는다. 근거는 [`docs/adr/0008`](../../docs/adr/0008-getrandom-wasm-shim.md).)
#[derive(Debug, Clone)]
pub struct Scheduler {
    fsrs: FSRS,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// 한 Rating을 눌렀을 때의 도착 지점.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScheduledState {
    pub memory: Memory,
    pub interval_days: f32,
    pub state: ReviewState,
    pub due_at: OffsetDateTime,
}

impl ScheduledState {
    /// 이번 세션 안에서 다시 보여줘야 하는가.
    pub fn is_intra_session(&self) -> bool {
        self.interval_days < 1.0
    }

    /// 이 도착 지점을 사람이 읽는 문장으로.
    ///
    /// **여기가 정본이다.** 서버가 버튼 라벨을 만들고 화면도 라벨을 그린다. 두 곳이
    /// 따로 만들면 어느 한쪽만 고쳐진다 — 서버는 3일이라 버리는데 화면은 "2일 후"라
    /// 쓰게 된다. 문장 규칙이 하나의 함수에만 있어야 그 일이 없다.
    pub fn label(&self) -> String {
        interval_label(self.interval_days.max(0.0).round() as u32)
    }
}

/// 일수를 사람이 읽는 문장으로.
///
/// `0` 이면 "지금", `1` 이면 "내일". 사용자에게 "0일 후" 라고 말하면 아무것도
/// 아니다 — 곧 다시 본다는 뜻이 아니라 아무것도 아니다.
pub fn interval_label(days: u32) -> String {
    match days {
        0 => "지금".to_owned(),
        1 => "내일".to_owned(),
        d => format!("{d}일 후"),
    }
}

/// 네 Rating에 대한 도착 지점을 한 번에 미리 보는 것.
///
/// Study 화면에서 버튼 옆에 "3일 후"를 보여줄 때 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Preview {
    pub again: ScheduledState,
    pub hard: ScheduledState,
    pub good: ScheduledState,
    pub easy: ScheduledState,
}

impl Preview {
    pub fn get(&self, rating: Rating) -> &ScheduledState {
        match rating {
            Rating::Again => &self.again,
            Rating::Hard => &self.hard,
            Rating::Good => &self.good,
            Rating::Easy => &self.easy,
        }
    }
}

impl Scheduler {
    /// 평균적인 학습 습관을 담은 기본 파라미터로 시작한다.
    ///
    /// 사용자별 파라미터는 `review_log`를 모아 네이티브 잡에서 학습한 뒤 교체한다.
    /// 그때까지 기본값을 쓴다.
    pub fn new() -> Self {
        Self {
            fsrs: FSRS::default(),
        }
    }

    /// 파라미터를 직접 지정한 스케줄러. 사용자별 학습 결과를 주입할 때 쓴다.
    pub fn with_parameters(parameters: &[f32]) -> Result<Self, ScheduleError> {
        FSRS::new(parameters)
            .map(|fsrs| Self { fsrs })
            .map_err(|_| ScheduleError::Rejected)
    }

    /// 4개 Rating이 각각 어디로 보내는지 미리 본다.
    ///
    /// `now`는 `current.due_at` 이후의 시각이어야 한다. 여기서는 다음 복습 시각을
    /// 계산하기 위한 기준점으로만 쓰며, `current`의 값은 바꾸지 않는다.
    pub fn preview(
        &self,
        current: &MemoryState,
        preset: RetentionPreset,
        now: OffsetDateTime,
    ) -> Result<Preview, ScheduleError> {
        let elapsed = elapsed_days(current, now);
        let next = self
            .fsrs
            .next_states(to_fsrs(current.memory), preset.desired_retention(), elapsed)
            .map_err(|_| ScheduleError::Rejected)?;

        Ok(Preview {
            again: to_scheduled(&next.again, now),
            hard: to_scheduled(&next.hard, now),
            good: to_scheduled(&next.good, now),
            easy: to_scheduled(&next.easy, now),
        })
    }

    /// Review 하나를 반영해 다음 [`MemoryState`]를 만든다.
    ///
    /// `reviewed_at`는 실제 복습 시각이다. `current.last_review_at`와의 차이로
    /// 경과 일수를 계산한다.
    pub fn apply(
        &self,
        current: &MemoryState,
        rating: Rating,
        preset: RetentionPreset,
        reviewed_at: OffsetDateTime,
    ) -> Result<MemoryState, ScheduleError> {
        let preview = self.preview(current, preset, reviewed_at)?;
        let next = *preview.get(rating);

        Ok(MemoryState {
            state: ReviewState::Review,
            memory: Some(next.memory),
            elapsed_days: next.interval_days,
            scheduled_days: next.interval_days,
            due_at: next.due_at,
            last_review_at: Some(reviewed_at),
            reps: current.reps.saturating_add(1),
            lapses: current.lapses + u32::from(rating == Rating::Again),
        })
    }
}

fn to_scheduled(item: &fsrs::ItemState, from: OffsetDateTime) -> ScheduledState {
    // `f32::max`는 NaN이면 0.0을 돌려준다. 그래도 saturating 계산으로 한 번 더 방어한다.
    let interval_days = item.interval.max(0.0);
    let mut interval = Duration::saturating_seconds_f32(interval_days * 86_400.0);

    if interval_days < 1.0 {
        // 세션 안에서 다시 보여줄 수 있도록 최소 간격을 둔다.
        interval = interval.max(MIN_INTRA_SESSION_INTERVAL);
    }

    ScheduledState {
        memory: Memory {
            stability: item.memory.stability,
            difficulty: item.memory.difficulty,
        },
        interval_days,
        state: ReviewState::Review,
        // 시각 덧셈이 넘치면 한계 시각에 붙인다. 조용히 패닉하지 않는 편이 낫다.
        due_at: from.saturating_add(interval),
    }
}

fn to_fsrs(memory: Option<Memory>) -> Option<FsrsMemory> {
    memory.map(|m| FsrsMemory {
        stability: m.stability,
        difficulty: m.difficulty,
    })
}

/// 마지막 복습부터 `now`까지 경과한 일수.
///
/// `fsrs`의 `days_elapsed`는 `u32`라 소수점을 담을 수 없다. 여기서 내림한다 —
/// 12시간 지났으면 0일로 본다. 0.5일 차이는 어떤 구간에서도 무시할 만하지만
/// 실제 오차가 어느 방향인지 알고 있어야 하므로 여기 적어 둔다.
fn elapsed_days(current: &MemoryState, now: OffsetDateTime) -> u32 {
    let Some(last) = current.last_review_at else {
        return 0;
    };
    let delta = now - last;
    let days = delta.whole_days();
    u32::try_from(days.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn new_state() -> MemoryState {
        MemoryState::new(datetime!(2026-03-02 09:00:00 UTC))
    }

    fn at(days: i64) -> OffsetDateTime {
        datetime!(2026-03-02 09:00:00 UTC) + Duration::days(days)
    }

    #[test]
    fn a_new_card_failing_comes_back_inside_the_session() {
        // FSRS는 새 단어의 망각을 수 분 단위로 되돌린다. 학습자가 "또 모름"을 눌렀을 때
        // 바로 다시 보여주는 경험은 여기서 나온다. 크레이트 문서의 예시와 같다:
        // again=0.212, hard=1.2931, good=2.3065, easy=8.2956 (일 단위).
        let scheduler = Scheduler::new();
        let preview = scheduler
            .preview(&new_state(), RetentionPreset::Balanced, at(0))
            .unwrap();

        assert!(
            preview.again.is_intra_session(),
            "a new card should come back this session, got {:?} days",
            preview.again.interval_days
        );
        assert!(
            preview.good.interval_days >= 1.0,
            "Good on a new card should land tomorrow or later, got {:?}",
            preview.good.interval_days
        );
    }

    #[test]
    fn a_matured_card_failing_waits_days_not_minutes() {
        // 여기가 순수 FSRS-6의 실제 동작이다. **성숙한 단어를 잊으면 오늘 다시 보여주지
        // 않는다.** 2~3일 뒤에 다시 만난다. Anki의 learning step이 만드는 "10분 후 재도전"
        // 경험은 우리가 채택하지 않은 그 모델에만 존재한다.
        //
        // 제품 함의: Study 화면이 "또 모름"을 눌렀을 때 즉각 재제시하지 않는 것을
        // 사용자가 버그로 보지 않고 정상으로 보게 해야 한다. 되돌리려면 learning step을
        // 도입해야 하는데, 그건 XP 1:1 대응을 깨뜨린다.
        let scheduler = Scheduler::new();
        let mut state = new_state();
        state.last_review_at = Some(at(-30));
        state.memory = Some(Memory {
            stability: 30.0,
            difficulty: 5.0,
        });

        let preview = scheduler
            .preview(&state, RetentionPreset::Balanced, at(0))
            .unwrap();

        assert!(
            !preview.again.is_intra_session(),
            "a matured card's lapse should be scheduled days out, got {:?} days",
            preview.again.interval_days
        );
        assert!(
            !preview.easy.is_intra_session(),
            "a solid recall should push the card out, got {:?}",
            preview.easy.interval_days
        );
    }

    #[test]
    fn ratings_order_the_intervals_again_hard_good_easy() {
        let scheduler = Scheduler::new();
        let mut state = new_state();
        state.last_review_at = Some(at(-10));
        state.memory = Some(Memory {
            stability: 8.0,
            difficulty: 5.0,
        });

        let preview = scheduler
            .preview(&state, RetentionPreset::Balanced, at(0))
            .unwrap();

        let intervals = [
            preview.again.interval_days,
            preview.hard.interval_days,
            preview.good.interval_days,
            preview.easy.interval_days,
        ];
        for pair in intervals.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "intervals must be non-decreasing across ratings: {intervals:?}"
            );
        }
    }

    #[test]
    fn intra_session_cards_never_come_back_instantly() {
        let scheduler = Scheduler::new();
        let mut state = new_state();
        state.last_review_at = Some(at(-365));
        state.memory = Some(Memory {
            stability: 365.0,
            difficulty: 1.0,
        });

        let preview = scheduler
            .preview(&state, RetentionPreset::Balanced, at(0))
            .unwrap();
        let again = preview.again;

        assert!(
            again.due_at > at(0),
            "due_at must be strictly in the future"
        );
        assert!(again.due_at - at(0) >= MIN_INTRA_SESSION_INTERVAL);
    }

    #[test]
    fn apply_records_the_review_and_advances_the_state() {
        let scheduler = Scheduler::new();
        let start = new_state();
        let reviewed_at = at(0);

        let next = scheduler
            .apply(&start, Rating::Good, RetentionPreset::Balanced, reviewed_at)
            .unwrap();

        assert_eq!(next.reps, 1);
        assert_eq!(next.lapses, 0);
        assert_eq!(next.state, ReviewState::Review);
        assert_eq!(next.last_review_at, Some(reviewed_at));
        assert!(next.memory.is_some());
        assert!(next.due_at > reviewed_at);
    }

    #[test]
    fn apply_counts_a_lapse_only_for_again() {
        let scheduler = Scheduler::new();
        let mut state = new_state();
        state.last_review_at = Some(at(-5));
        state.memory = Some(Memory {
            stability: 5.0,
            difficulty: 5.0,
        });

        for (rating, expected) in [
            (Rating::Again, 1),
            (Rating::Hard, 0),
            (Rating::Good, 0),
            (Rating::Easy, 0),
        ] {
            let next = scheduler
                .apply(&state, rating, RetentionPreset::Balanced, at(0))
                .unwrap();
            assert_eq!(next.lapses, expected, "rating {rating:?}");
            assert_eq!(next.reps, state.reps + 1);
        }
    }

    #[test]
    fn higher_retention_shortens_intervals() {
        let scheduler = Scheduler::new();
        let mut state = new_state();
        state.last_review_at = Some(at(-40));
        state.memory = Some(Memory {
            stability: 40.0,
            difficulty: 5.0,
        });

        let diligent = scheduler
            .preview(&state, RetentionPreset::Diligent, at(0))
            .unwrap();
        let frugal = scheduler
            .preview(&state, RetentionPreset::Frugal, at(0))
            .unwrap();

        assert!(
            diligent.good.interval_days < frugal.good.interval_days,
            "부지런한 설정이 더 자주 복습하게 해야 한다: {} vs {}",
            diligent.good.interval_days,
            frugal.good.interval_days
        );
    }

    #[test]
    fn interval_never_goes_backwards() {
        let scheduler = Scheduler::new();
        let mut state = new_state();
        let mut reviewed_at = at(0);

        for _ in 0..12 {
            let next = scheduler
                .apply(&state, Rating::Good, RetentionPreset::Balanced, reviewed_at)
                .unwrap();
            state = next;
            reviewed_at += Duration::days(f64::from(state.scheduled_days.round()) as i64);
        }

        assert!(
            state.scheduled_days > 1.0,
            "12 successful reviews should push past a day, got {:?}",
            state.scheduled_days
        );
    }

    #[test]
    fn the_scheduler_is_deterministic() {
        let scheduler = Scheduler::new();
        let mut state = new_state();
        state.last_review_at = Some(at(-17));
        state.memory = Some(Memory {
            stability: 17.0,
            difficulty: 4.0,
        });

        let first = scheduler
            .preview(&state, RetentionPreset::Balanced, at(0))
            .unwrap();
        for _ in 0..5 {
            let again = scheduler
                .preview(&state, RetentionPreset::Balanced, at(0))
                .unwrap();
            assert_eq!(first, again, "preview must not vary between calls");
        }
    }

    #[test]
    fn elapsed_days_before_the_last_review_is_clamped() {
        // 시계가 뒤로 간 경우. 0일로 보고 부호가 뒤집히지 않아야 한다.
        let mut state = new_state();
        state.last_review_at = Some(at(10));
        let scheduler = Scheduler::new();
        let result = scheduler.preview(&state, RetentionPreset::Balanced, at(0));
        assert!(result.is_ok());
    }
}

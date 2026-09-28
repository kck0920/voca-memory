//! Voca Memory의 도메인 로직.
//!
//! 이 크레이트는 **서버와 브라우저 양쪽에서 그대로 실행 가능해야 한다.**
//! `async`를 노출하지 않고, I/O를 하지 않으며, 스케줄링 계산을 순수 함수로 유지한다.
//! 근거는 [`docs/adr/0003`](../../docs/adr/0003-voca-domain-is-wasm-safe-pure-rust.md).
//!
//! 외부 I/O(사전 조회, 저장소, 인증)는 [`voca-store`], [`voca-dict`],
//! [`voca-server`]가 담당한다. 이 크레이트는 그런 것들을 알지 못한다.

mod clock;
mod gamify;
mod id;
mod lexicon;
mod schedule;
mod streak;
mod study;

pub use clock::{MAX_ABS_UTC_OFFSET_SECONDS, clamp_utc_offset_seconds, local_date};
pub use gamify::{
    LevelProgress, RetentionPreset, XP_PER_RATING, level_for, xp_for, xp_for_next_level,
    xp_progress,
};
pub use id::Id;
pub use lexicon::{Sense, SenseKind, SenseSource, Word, WordSource};
pub use schedule::{Preview, ScheduleError, ScheduledState, Scheduler, interval_label};
pub use streak::{Streak, StreakUpdate};
pub use study::{Card, Deck, Memory, MemoryState, Rating, Review, ReviewState, SessionSummary};

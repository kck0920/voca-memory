//! 저장소 seam.
//!
//! 여기 있는 것은 **interface**다. 구현은 `voca-store-sqlite`가 채운다.
//!
//! 이 모듈은 두 가지를 한곳에 가둔다:
//!
//! 1. **트랜잭션 경계.** `submit_review` 하나가 `review_log` 삽입, `card_states` 갱신,
//!    `streaks` 재계산, `xp_events` 삽입을 한 트랜잭션으로 한다. 호출부가 이 네
//!    개를 올바른 순서로 조합해야 한다는 사실을 알면 안 된다.
//! 2. **파생 규칙.** Streak, XP, Level, 회수 가능성 계산은 여기서 나온다.
//!    호출부가 `voca-domain`의 순수 함수를 직접 부르지 않는다.
//!
//! ## 왜 테이블 CRUD가 아니라 도메인 연산인가
//!
//! `CardRepository { insert, update, delete }` 같은 인터페이스는 이 모듈을 지워도
//! 복잡성이 사라지지 않는다 — 그 CRUD는 어차피 sqlx가 이미 준 것이다. 그런 인터페이스는
//! 얕다. 진짜 비용은 `review_log`/`card_states`/`streaks`/`xp_events` 네 행을
//! **반드시 함께** 갱신하는 데 있고, 그것은 이 모듈만 안다.
//!
//! ## 동기화
//!
//! 동기화는 이 interface를 **다른 방향으로** 지난다. `changes_since`이 `rev` 이후의
//! 변경을 tombstones까지 포함해 돌려준다. 일반 조회 경로는 soft delete를 걸러내므로
//! 호출자는 `deleted_at`의 존재를 모른다.

mod account;
mod change;
mod dashboard;
mod deck;
mod error;
mod lexicon;
mod query;
mod review;
mod revision;
mod session;

pub use account::{
    Accounts, AuthFailure, Authenticated, Credential, DEFAULT_RETENTION, NewAccount,
};
pub use change::{
    CardRow, Change, ChangePage, ChangeRequest, DeckRow, ReviewSummary, SenseRow, StateRow, WordRow,
};
pub use dashboard::{Dashboard, DayReviewStat, DeckProgress};
pub use deck::{AddCards, CardView, DeckUpdate, DeckView, NewDeck, UserCardDetail};
pub use error::{StoreError, StoreResult};
pub use lexicon::{
    DictionarySense, NewUserSense, SenseKind, SenseSource, SenseView, UpsertDictionaryWord,
    WordSource, WordView,
};
pub use query::{Page, SenseQuery};
pub use review::{
    DeckViewCardRef, MAX_CONFLICT_RETRIES, QueuedCard, ReviewOutcome, ReviewRequest, StudyQueue,
    StudyRequest,
};
pub use revision::{Revision, Versioned};
pub use session::{
    AuthResult, LoginRequest, RegisterRequest, SessionGrant, SessionToken, Sessions,
};

use std::future::Future;

use voca_domain::Id;

pub type UserId = Id;
pub type DeckId = Id;
pub type CardId = Id;
pub type SenseId = Id;
pub type WordId = Id;
pub type ReviewId = Id;

/// 저장소가 만족해야 하는 인터페이스.
///
/// **구현이 하나뿐이어도 이 seam을 유지한다.** 유지하는 이유는 교체 지점을 한 곳에
/// 모으기 위해서다 — 서버를 Turso로 옮길 때, 그리고 저장소 규칙을 오프라인 경로에서
/// 그대로 재사용할 때. 근거는 [`docs/adr/0010`](../../docs/adr/0010-server-db-is-sqlite-on-a-single-vps.md).
///
/// 모든 메서드는 `&self`다. 구현이 내부적으로 풀(mutex)이나 트랜잭션을 가진다.
/// 호출자는 동시성 primitive를 다루지 않는다.
///
/// ## `async fn`이 아니라 RPITIT을 쓰는 이유
///
/// `async fn`을 trait에 쓰면 반환 future의 auto trait을 선언할 수 없다. 그 결과
/// axum 핸들러 안에서 쓸 때 "future is not `Send`"가 된다. 여기서는
/// `impl Future<Output = ..> + Send`를 명시해 그 문제를 interface에서 없앤다.
/// `#[async_trait]`로 감싸면 `Box<dyn Future>` 할당이 생기므로 그것도 쓰지 않는다.
pub trait Store: Send + Sync {
    // ── 사전 ────────────────────────────────────────────────
    /// 표기형으로 단어를 찾는다. 사전 단어와 사용자 단어가 함께 나온다.
    fn find_word(
        &self,
        user: UserId,
        lemma: &str,
    ) -> impl Future<Output = StoreResult<Option<WordView>>> + Send;

    /// 뜻 단위로 검색한다. 대시보드의 "단어 추가"와 사전 팝업이 이 경로를 쓴다.
    fn search_senses(
        &self,
        user: UserId,
        query: &SenseQuery,
    ) -> impl Future<Output = StoreResult<Page<SenseView>>> + Send;

    /// 학습자가 직접 지은 Sense를 저장한다. 같은 표기형의 Sense가 이미 있으면 그
    /// Word 아래에 붙이고, 없으면 사용자 Word를 함께 만든다.
    fn put_user_sense(
        &self,
        user: UserId,
        input: NewUserSense,
    ) -> impl Future<Output = StoreResult<SenseView>> + Send;

    /// Sense를 숨긴다. 물리 삭제가 아니고 `archived_at`을 찍는다 — 동기화 대상이기
    /// 때문이다. 사용자에게 보이는 목록에서는 사라진다.
    fn archive_sense(
        &self,
        user: UserId,
        sense: SenseId,
    ) -> impl Future<Output = StoreResult<()>> + Send;

    // ── 덱 ──────────────────────────────────────────────────
    fn list_decks(&self, user: UserId) -> impl Future<Output = StoreResult<Vec<DeckView>>> + Send;

    fn create_deck(
        &self,
        user: UserId,
        input: NewDeck,
    ) -> impl Future<Output = StoreResult<DeckView>> + Send;

    fn update_deck(
        &self,
        user: UserId,
        input: DeckUpdate,
    ) -> impl Future<Output = StoreResult<DeckView>> + Send;

    // ── 카드 ────────────────────────────────────────────────
    /// 덱에 Sense들을 Card로 넣는다. 같은 Sense가 이미 있으면 넣지 않는다.
    fn add_cards(
        &self,
        user: UserId,
        deck: DeckId,
        senses: &[SenseId],
    ) -> impl Future<Output = StoreResult<Vec<CardView>>> + Send;

    /// Card를 다른 덱으로 복제한다. 복제본은 복습 이력을 **가져오지 않는다** —
    /// `New` 상태로 시작한다 ([`docs/adr/0004`](../../docs/adr/0004-word-sense-card-deck-model.md)).
    fn clone_card(
        &self,
        user: UserId,
        card: CardId,
        to_deck: DeckId,
    ) -> impl Future<Output = StoreResult<CardView>> + Send;

    fn archive_card(
        &self,
        user: UserId,
        card: CardId,
    ) -> impl Future<Output = StoreResult<()>> + Send;

    /// 사용자의 카드 목록을 상세 정보(단어, 품사, 뜻, 스케줄링 상태)와 함께 조회한다.
    fn list_cards(
        &self,
        user: UserId,
        deck: Option<DeckId>,
    ) -> impl Future<Output = StoreResult<Vec<UserCardDetail>>> + Send;

    /// 최근 N일간의 일별 복습 횟수 통계를 조회한다.
    fn review_stats(
        &self,
        user: UserId,
        days: u32,
    ) -> impl Future<Output = StoreResult<Vec<DayReviewStat>>> + Send;

    // ── 복습 ────────────────────────────────────────────────
    /// 지금 풀 Card를 고른다. 신규는 덱의 `new_per_day`를, 복습은 회수 가능성이
    /// 낮은 순으로 정렬한다. 이 interface 하나에 정렬 정책과 일일 한도가 함께 들어 있다.
    fn study_queue(
        &self,
        request: StudyRequest,
    ) -> impl Future<Output = StoreResult<StudyQueue>> + Send;

    /// Review 하나를 반영한다. **원자적이다.** 다음 복습 시각, 4개 Rating의 도착
    /// 지점, 갱신된 Streak, 얻은 XP와 Level을 한 번에 돌려준다.
    fn submit_review(
        &self,
        request: ReviewRequest,
    ) -> impl Future<Output = StoreResult<ReviewOutcome>> + Send;

    // ── 대시보드 ────────────────────────────────────────────
    fn dashboard(
        &self,
        user: UserId,
        now: time::OffsetDateTime,
    ) -> impl Future<Output = StoreResult<Dashboard>> + Send;

    // ── 동기화 ──────────────────────────────────────────────
    /// `since` revision 이후 바뀐 행을 tombstones까지 포함해 돌려준다.
    /// 이 경로만 soft delete된 행을 그대로 노출한다.
    fn changes_since(
        &self,
        request: ChangeRequest,
    ) -> impl Future<Output = StoreResult<ChangePage>> + Send;
}

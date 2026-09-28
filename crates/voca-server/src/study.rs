//! 복습 화면이 필요로 하는 HTTP 경로.
//!
//! **정렬도, 스케줄 계산도, 트랜잭션도 여기 없다.** [`voca_store::Store`] 가
//! 그 일을 하고 여기서는 왕복만 한다. 핸들러가 하나라도 그 규칙을 복사하면 두
//! 구현이 어긋난다 — 화면의 버튼 라벨이 서버의 실제 예약 시각과 달라지는 일이
//! 그 결과다.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use voca_domain::{Rating, ReviewState, interval_label};
use voca_store::{CardId, DeckId, ReviewRequest, StudyRequest};

use crate::AppState;
use crate::routes::{ApiError, LoggedIn, require_login};
use voca_store::Store;

/// 한 번에 몇 장까지 줄지.
///
/// 클라이언트가 정할 수 있게 두면 화면 한 번에 수천 장을 잡는다. 서버가 정한다.
const MAX_QUEUE_LIMIT: u32 = 100;
const DEFAULT_QUEUE_LIMIT: u32 = 20;

// ── 복습 큐 ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StudyBody {
    pub cards: Vec<QueuedCardBody>,
    pub reviews_remaining: u32,
    pub new_remaining_today: u32,
}

/// 큐에 든 Card 한 장.
///
/// `QueuedCard` 를 그대로 내보내지 않는다 — 화면이 못 쓰는 값(회수 가능성 등)을
/// 숨기고, **4개 버튼 라벨을 미리 계산해 넣는다.** 라벨 계산은
/// `voca-ui` 가 아니라 서버가 한다. 서버가 모르는 값으로 화면을 그리면 실제
/// 예약 시각과 어긋난다.
#[derive(Debug, Serialize)]
pub struct QueuedCardBody {
    pub card_id: String,
    pub deck_id: String,
    pub lemma: String,
    pub phonetic: Option<String>,
    /// Card Front. Sense의 종류가 정한다 — `example` 은 예문을 보여준다.
    pub front: CardFront,
    /// 뜻풀이. 뒷면.
    pub definition: String,
    pub pos: Option<String>,
    pub example_ko: Option<String>,
    pub is_new: bool,
}

#[derive(Debug, Serialize)]
// 바깥 형식은 전부 소문자로 맞춘다. Rust 의 `Lemma` 가 그대로 나가면 화면 쪽
// 문자열 비교가 어긋난다 — 대소문자 규칙은 바깥 형식의 한 곳에만 둔다.
#[serde(tag = "kind", content = "text", rename_all = "lowercase")]
pub enum CardFront {
    /// `word` / `phrase` — 그 Word의 표기형.
    Lemma(String),
    /// `example` — 그 Sense의 예문.
    Example(String),
}

pub async fn study_queue(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<QueueQuery>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;

    let limit = q
        .limit
        .unwrap_or(DEFAULT_QUEUE_LIMIT)
        .clamp(1, MAX_QUEUE_LIMIT);
    // 깨진 deck id는 **무시가 아니라 400이다.** `None`으로 바꾸면 "전체 덱"이
    // 되어 사용자가 고르지도 않은 덱의 카드가 나온다 — 조용한 오답이 조용한
    // 실패보다 나쁘다. `submit_review`의 깨진 `card_id` 처리와 같은 규칙이다.
    let deck = match q.deck.as_deref() {
        None => None,
        Some(raw) => Some(parse_deck_id(raw).ok_or_else(|| {
            ApiError::from(voca_store::AuthFailure::Rejected("덱을 고르지 못했다"))
        })?),
    };
    let request = StudyRequest {
        user: user.user_id,
        deck,
        now: OffsetDateTime::now_utc(),
        limit,
    };

    let queue = state
        .store()
        .study_queue(request)
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(StudyBody {
        cards: queue.cards.iter().map(card_body).collect(),
        reviews_remaining: queue.reviews_remaining,
        new_remaining_today: queue.new_remaining_today,
    })
    .into_response())
}

#[derive(Debug, Deserialize)]
pub struct QueueQuery {
    pub deck: Option<String>,
    pub limit: Option<u32>,
}

// ── Review ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SubmitBody {
    pub card_id: String,
    pub rating: Rating,
    /// 학습자가 화면에서 본 시간(밀리초). 선택.
    #[serde(default)]
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    pub card_id: String,
    /// 반영 후의 다음 복습 시각 (ISO 8601).
    pub due_at: String,
    pub scheduled_days: f32,
    /// 4개 Rating이 각각 어디로 보내는지. 버튼 라벨용.
    pub preview: PreviewBody,
    pub streak: StreakBody,
    pub xp_earned: u32,
    pub level: LevelBody,
    /// 이 Review 뒤에도 같은 세션에서 다시 보여줘야 하는가.
    pub requeue: bool,
}

/// Rating 하나가 만드는 도착 지점.
#[derive(Debug, Serialize)]
pub struct RatingPreview {
    /// "지금" / "내일" / "3일 후"
    pub label: String,
    /// 화면이 긋는 데 쓰는 일수.
    pub days: u32,
}

#[derive(Debug, Serialize)]
pub struct PreviewBody {
    pub again: RatingPreview,
    pub hard: RatingPreview,
    pub good: RatingPreview,
    pub easy: RatingPreview,
}

#[derive(Debug, Serialize)]
pub struct StreakBody {
    pub current: u32,
    pub longest: u32,
    /// 오늘 Review가 있는가. Streak 숫자와는 별개다.
    pub reviewed_today: bool,
}

#[derive(Debug, Serialize)]
pub struct LevelBody {
    pub level: u32,
    pub xp_into_level: u32,
    pub xp_span: u32,
}

pub async fn submit_review(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::Json<SubmitBody>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    state.require_same_origin(&headers)?;

    let body = body.0;
    let Some(card) = parse_card_id(&body.card_id) else {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "카드를 고르지 못했다",
        )));
    };

    // Streak 판정의 기준이 되는 현지 날짜. 서버가 한 번 정하고 그 값으로 끝낸다 —
    // 요청 처리 중에 자정이 지나면 `reviewed_at`과 `local_date`가 서로 다른 날이
    // 되어 Streak가 하루 어긋난다. 오프셋까지 같은 순간에서 뽑는다.
    let (now, offset, local_date) = local_moment();

    let request = ReviewRequest {
        user: user.user_id,
        card,
        rating: body.rating,
        reviewed_at: now,
        local_date,
        utc_offset_seconds: offset.whole_seconds(),
        duration_ms: body.duration_ms,
    };

    let outcome = state
        .store()
        .submit_review(request)
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(SubmitResponse {
        card_id: outcome.card_id.to_string(),
        due_at: outcome
            .memory_state
            .due_at
            .to_offset(offset)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        scheduled_days: outcome.memory_state.scheduled_days,
        preview: PreviewBody {
            again: preview_body(&outcome.preview.again),
            hard: preview_body(&outcome.preview.hard),
            good: preview_body(&outcome.preview.good),
            easy: preview_body(&outcome.preview.easy),
        },
        streak: StreakBody {
            current: outcome.streak.current,
            longest: outcome.streak.longest,
            reviewed_today: outcome.streak.last_review_date == Some(local_date),
        },
        xp_earned: outcome.xp_earned,
        level: LevelBody {
            level: outcome.level.level,
            xp_into_level: outcome.level.xp_into_level,
            xp_span: outcome.level.xp_span,
        },
        requeue: outcome.requeues_this_session(),
    })
    .into_response())
}

fn preview_body(s: &voca_domain::ScheduledState) -> RatingPreview {
    let days = s.interval_days.max(0.0).round() as u32;
    RatingPreview {
        label: interval_label(days),
        days,
    }
}

// ── 대시보드 ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DashboardBody {
    pub streak: StreakBody,
    pub level: LevelBody,
    pub reviews_due: u32,
    pub new_remaining: u32,
    pub decks: Vec<DashboardDeckBody>,
    /// 할 게 남았는데 오늘 아직 안 했다.
    pub streak_at_risk: bool,
}

#[derive(Debug, Serialize)]
pub struct DashboardDeckBody {
    pub deck_id: String,
    pub total: u32,
    pub seen: u32,
    pub due: u32,
    pub fresh: u32,
    pub progress_percent: u32,
}

pub async fn dashboard(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;

    let d = state
        .store()
        .dashboard(user.user_id, OffsetDateTime::now_utc())
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(DashboardBody {
        streak: StreakBody {
            current: d.streak.current,
            longest: d.streak.longest,
            reviewed_today: d.streak.last_review_date == Some(d.local_date),
        },
        level: LevelBody {
            level: d.level.level,
            xp_into_level: d.level.xp_into_level,
            xp_span: d.level.xp_span,
        },
        reviews_due: d.reviews_due,
        new_remaining: d.new_remaining,
        decks: d
            .decks
            .iter()
            .map(|p| DashboardDeckBody {
                deck_id: p.id.to_string(),
                total: p.total,
                seen: p.seen,
                due: p.due,
                fresh: p.fresh,
                progress_percent: p.progress_percent(),
            })
            .collect(),
        streak_at_risk: d.is_streak_at_risk(),
    })
    .into_response())
}

// ── 보조 ─────────────────────────────────────────────────

/// Card Front 를 정한다.
///
/// **여기서 정한다, 화면에서 정하지 않는다.** Sense 의 종류가 정한다는 규칙이
/// 서버와 화면에 두 번 있으면 한쪽만 고쳐진다. 서버가 정해 보내면 화면은 그리기만
/// 한다.
fn card_body(c: &voca_store::QueuedCard) -> QueuedCardBody {
    use voca_domain::SenseKind;

    let front = match c.sense.kind {
        SenseKind::Word | SenseKind::Phrase => CardFront::Lemma(c.word.lemma.clone()),
        // 예문 Sense 의 Word 는 항상 있다. 없으면 버린다 — 없는 값을 지어내지 않는다.
        SenseKind::Example => match c.sense.example_en.as_deref() {
            Some(e) if !e.trim().is_empty() => CardFront::Example(e.to_owned()),
            _ => CardFront::Lemma(c.word.lemma.clone()),
        },
    };

    QueuedCardBody {
        card_id: c.card.card_id.to_string(),
        deck_id: c.card.deck_id.to_string(),
        lemma: c.word.lemma.clone(),
        phonetic: c.word.phonetic.clone(),
        front,
        definition: c.sense.definition.clone(),
        pos: c.sense.pos.clone(),
        example_ko: c.sense.example_ko.clone(),
        is_new: c.memory_state.state == ReviewState::New,
    }
}

fn parse_card_id(raw: &str) -> Option<CardId> {
    raw.parse::<uuid::Uuid>().ok().map(CardId::from)
}

fn parse_deck_id(raw: &str) -> Option<DeckId> {
    raw.parse::<uuid::Uuid>().ok().map(DeckId::from)
}

/// 요청 순간의 현지 시각.
///
/// Streak 판정은 **서버의** 날짜로 한다. 클라이언트가 자기 날짜를 보내면 시계를
/// 조작해 Streak를 늘릴 수 있다. `now`와 `offset`을 따로 읽으면 자정 경계에서
/// 어긋나므로, 한 번 읽은 순간에서 둘 다 뽑는다.
fn local_moment() -> (OffsetDateTime, time::UtcOffset, time::Date) {
    let now = OffsetDateTime::now_utc();
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let date = now.to_offset(offset).date();
    (now, offset, date)
}

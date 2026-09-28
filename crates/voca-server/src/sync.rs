//! 동기화.
//!
//! **여러 기기가 한 계정을 쓰는 유일한 길이 여기다.** 데스크톱에서 외워 두고 폰에서
//! 보면 그게 성립해야 한다.
//!
//! 클라이언트는 `since` revision 이후의 변경만 받고, 다 적용한 뒤 `watermark` 를
//! 저장한다. 아무것도 안 바뀌었어도 **watermark 는 갱신된다** — 그 사실도 갱신이다.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use voca_store::{Change, ChangePage, ChangeRequest, Store, Versioned};

use crate::AppState;
use crate::routes::{ApiError, LoggedIn, require_login};

/// 한 번에 몇 건까지.
///
/// 클라이언트가 정하게 두면 한 번에 수천 건을 받아 메모리를 잡는다. 상한은
/// 서버가 정한다. 넘으면 `has_more` 로 알려주고 같은 `since` 로 다시 부른다.
const MAX_LIMIT: u32 = 1_000;

#[derive(Debug, Serialize)]
pub struct SyncBody {
    pub changes: Vec<ChangeBody>,
    /// 이 페이지를 다 적용한 뒤 저장할 revision. 비어 있어도 갱신된다.
    pub watermark: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeBody {
    Card(Versioned<CardRowBody>),
    Deck(Versioned<DeckRowBody>),
    Sense(Versioned<SenseRowBody>),
    Word(Versioned<WordRowBody>),
    /// soft delete. **본문 없이 사라졌다는 사실만** 전한다 — 지워진 카드의 정의를
    /// 실어 보내지 않는다.
    CardTombstone {
        id: String,
        revision: u64,
    },
    DeckTombstone {
        id: String,
        revision: u64,
    },
    SenseTombstone {
        id: String,
        revision: u64,
    },
}

#[derive(Debug, Serialize)]
pub struct CardRowBody {
    pub id: String,
    pub deck_id: String,
    pub sense_id: String,
    /// 이 Card 를 어떤 Card 에서 복제했는가. **복제본은 이력 없이 시작한다**
    /// (docs/adr/0004) — 클라이언트가 실수로 이력을 물려받으면 안 된다.
    pub cloned_from: Option<String>,
    /// 스케줄러 상태. **Card 본문과 반드시 함께 온다.** 빠지면 기기를 바꿨을 때
    /// 복습 일정 전체가 뒤집힌다 — 여기에 0 을 넣어두면 조용히 망가진다.
    pub memory: StateBody,
}

#[derive(Debug, Serialize)]
pub struct StateBody {
    pub state: String,
    /// 아직 복습 전이면 `None` 다. 0 이 아니라 비어 있음 — 0 은 "안정성이 0" 이라는
    /// 뜻이라 다른 의미가 된다.
    pub stability: Option<f32>,
    pub difficulty: Option<f32>,
    pub elapsed_days: f32,
    pub scheduled_days: f32,
    /// Unix 초. ISO 문자열이 아니라 숫자를 보낸다 — 이 값은 사람이 읽지 않고
    /// 계산에만 쓰인다.
    pub due_at: i64,
    pub last_review_at: Option<i64>,
    pub reps: u32,
    pub lapses: u32,
}

#[derive(Debug, Serialize)]
pub struct DeckRowBody {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub daily_goal: u32,
    pub new_per_day: u32,
}

#[derive(Debug, Serialize)]
pub struct SenseRowBody {
    pub id: String,
    pub word_id: String,
    pub kind: String,
    pub source: String,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
    pub pos: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WordRowBody {
    pub id: String,
    pub lemma: String,
    pub source: String,
    pub phonetic: Option<String>,
    pub audio_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SyncQuery {
    /// 클라이언트가 마지막으로 본 revision. 없으면 0 — 전부를 받아라.
    #[serde(default)]
    pub since: u64,
    pub limit: Option<u32>,
}

pub async fn changes(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<SyncQuery>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;

    let page = state
        .store()
        .changes_since(ChangeRequest {
            user: user.user_id,
            since: q.since.into(),
            limit: q
                .limit
                .unwrap_or(ChangeRequest::DEFAULT_LIMIT)
                .clamp(1, MAX_LIMIT),
        })
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(sync_body(&page)).into_response())
}

fn sync_body(page: &ChangePage) -> SyncBody {
    SyncBody {
        changes: page.changes.iter().map(change_body).collect(),
        watermark: page.watermark.as_u64(),
        has_more: page.has_more,
    }
}

fn change_body(c: &Change) -> ChangeBody {
    match c {
        Change::Card(v) => ChangeBody::Card(Versioned::new(card_row(&v.item), v.revision)),
        Change::Deck(v) => ChangeBody::Deck(Versioned::new(deck_row(&v.item), v.revision)),
        Change::Sense(v) => ChangeBody::Sense(Versioned::new(sense_row(&v.item), v.revision)),
        Change::Word(v) => ChangeBody::Word(Versioned::new(word_row(&v.item), v.revision)),
        Change::CardTombstone { id, revision } => ChangeBody::CardTombstone {
            id: id.to_string(),
            revision: revision.as_u64(),
        },
        Change::DeckTombstone { id, revision } => ChangeBody::DeckTombstone {
            id: id.to_string(),
            revision: revision.as_u64(),
        },
        Change::SenseTombstone { id, revision } => ChangeBody::SenseTombstone {
            id: id.to_string(),
            revision: revision.as_u64(),
        },
    }
}

fn card_row(r: &voca_store::CardRow) -> CardRowBody {
    CardRowBody {
        id: r.id.to_string(),
        deck_id: r.deck_id.to_string(),
        sense_id: r.sense_id.to_string(),
        cloned_from: r.cloned_from.map(|c| c.to_string()),
        memory: StateBody {
            state: r.state.state.clone(),
            stability: r.state.stability,
            difficulty: r.state.difficulty,
            elapsed_days: r.state.elapsed_days,
            scheduled_days: r.state.scheduled_days,
            due_at: r.state.due_at,
            last_review_at: r.state.last_review_at,
            reps: r.state.reps,
            lapses: r.state.lapses,
        },
    }
}

fn deck_row(r: &voca_store::DeckRow) -> DeckRowBody {
    DeckRowBody {
        id: r.id.to_string(),
        name: r.name.clone(),
        description: r.description.clone(),
        daily_goal: r.daily_goal,
        new_per_day: r.new_per_day,
    }
}

fn sense_row(r: &voca_store::SenseRow) -> SenseRowBody {
    SenseRowBody {
        id: r.id.to_string(),
        word_id: r.word_id.to_string(),
        kind: r.kind.as_str().to_owned(),
        source: r.source.as_str().to_owned(),
        definition: r.definition.clone(),
        example_en: r.example_en.clone(),
        example_ko: r.example_ko.clone(),
        pos: r.pos.clone(),
    }
}

fn word_row(r: &voca_store::WordRow) -> WordRowBody {
    WordRowBody {
        id: r.id.to_string(),
        lemma: r.lemma.clone(),
        source: r.source.as_str().to_owned(),
        phonetic: r.phonetic.clone(),
        audio_url: r.audio_url.clone(),
    }
}

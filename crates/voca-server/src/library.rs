//! 단어와 덱을 다루는 HTTP 경로.
//!
//! 학습자가 단어를 "넣는" 길이 전부 여기 있다. **사전 조회와 별개다** —
//! [`crate::routes::lookup_word`] 은 찾아오기만 하고, 이 경로가 저장한다.
//!
//! 그 규칙이 화면에도 그대로 간다: 조회로 얻은 뜻은 아직 어디에도 없고, 덱에
//! 넣는 순간 Card 가 된다. 그 사이에 아무것도 저장하지 않는다 —
//! `voca-store` 의 전역 사전 단어를 누가 만들었는지 추적할 수 없는 상태가 되기
//! 때문이다.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use voca_domain::SenseKind;
use voca_store::{NewDeck, NewUserSense, Store};

use crate::AppState;
use crate::routes::{ApiError, LoggedIn, require_login};

/// "단어 추가" 요청.
///
/// 직접 만든 Sense 다. 사전에서 가져온 것과 **다른 출처**로 저장된다 — 어느 쪽에서
/// 왔는지는 나중에 "내 뜻"과 "사전 뜻"을 나눠 보여 줄 때 쓴다.
#[derive(Debug, Deserialize)]
pub struct AddSenseBody {
    pub lemma: String,
    pub kind: SenseKind,
    #[serde(default)]
    pub example_en: Option<String>,
    pub definition: String,
    #[serde(default)]
    pub pos: Option<String>,
}

/// 저장 결과를 돌려준다.
///
/// **`lemma` 가 없다.** 이 응답을 만들 때 안다는 뜻이 아니기 때문이다 — 알면
/// 지어내야 한다. 요청자가 보낸 표기형을 이미 갖고 있고, 정규화된 값이 필요하면
/// `find_word` 로 다시 읽으면 된다.
#[derive(Debug, Serialize)]
pub struct SenseBody {
    pub sense_id: String,
    pub word_id: String,
    pub kind: SenseKind,
    pub definition: String,
    pub example_en: Option<String>,
    pub pos: Option<String>,
}

/// 학습자가 직접 지은 Sense 를 저장한다.
///
/// 같은 표기형이 이미 있으면 그 Word 아래에 붙고, 없으면 Word 도 함께 만든다.
pub async fn add_sense(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::Json<AddSenseBody>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    state.require_same_origin(&headers)?;

    let b = body.0;
    // 도메인 규칙을 여기서 다시 거르지 않는다 — `NewUserSense::validate` 가
    // 정본이다. 여기서 한 번 더 거르면 메시지가 갈라진다.
    let input = NewUserSense {
        lemma: b.lemma,
        kind: b.kind,
        example_en: b.example_en,
        definition: b.definition,
        pos: b.pos,
    };
    if input.validate().is_err() {
        // 도메인이 무엇이 틀렸는지는 안다. 하지만 그 문장을 그대로 사용자에게
        // 흘리면 규칙이 HTTP 계층으로 새어 나간다 — `Rejected` 가 정본을 못 다루는
        // 입력은 여기서 한 문장으로 모은다.
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "단어와 뜻을 확인해라",
        )));
    }

    let sense = state
        .store()
        .put_user_sense(user.user_id, input)
        .await
        .map_err(ApiError::store)?;

    Ok((
        axum::http::StatusCode::CREATED,
        axum::Json(SenseBody {
            sense_id: sense.id.to_string(),
            word_id: sense.word_id.to_string(),
            kind: sense.kind,
            definition: sense.definition,
            example_en: sense.example_en,
            pos: sense.pos,
        }),
    )
        .into_response())
}

// ── 덱 ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DeckBody {
    pub deck_id: String,
    pub name: String,
    pub daily_goal: u32,
    pub new_per_day: u32,
}

// 덱별 카드 수는 **이 응답에 없다.** 대시보드(`/api/dashboard`)가 준다. 여기서
// 따로 세면 두 값이 갈라질 수 있고, 어느 쪽이 맞는지 아무도 모른다.

pub async fn list_decks(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;

    let decks = state
        .store()
        .list_decks(user.user_id)
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(
        decks
            .iter()
            .map(|d| DeckBody {
                deck_id: d.id.to_string(),
                name: d.name.clone(),
                daily_goal: d.daily_goal,
                new_per_day: d.new_per_day,
            })
            .collect::<Vec<_>>(),
    )
    .into_response())
}

#[derive(Debug, Deserialize)]
pub struct NewDeckBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub daily_goal: Option<u32>,
    #[serde(default)]
    pub new_per_day: Option<u32>,
}

pub async fn create_deck(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::Json<NewDeckBody>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    state.require_same_origin(&headers)?;

    let b = body.0;
    if b.name.trim().is_empty() {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "덱 이름이 비어 있다",
        )));
    }

    let deck = state
        .store()
        .create_deck(
            user.user_id,
            NewDeck {
                name: b.name,
                description: b.description,
                daily_goal: b.daily_goal,
                new_per_day: b.new_per_day,
            },
        )
        .await
        .map_err(ApiError::store)?;

    Ok((
        axum::http::StatusCode::CREATED,
        axum::Json(DeckBody {
            deck_id: deck.id.to_string(),
            name: deck.name,
            daily_goal: deck.daily_goal,
            new_per_day: deck.new_per_day,
        }),
    )
        .into_response())
}

// ── Card ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddCardsBody {
    pub deck_id: String,
    pub sense_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CardBody {
    pub card_id: String,
    pub deck_id: String,
    pub sense_id: String,
}

pub async fn add_cards(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::Json<AddCardsBody>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    state.require_same_origin(&headers)?;

    let b = body.0;
    if b.sense_ids.is_empty() {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "넣을 Sense 가 없다",
        )));
    }
    // 한 번에 몇 장까지. 화면이 실수로 수천 장을 보내도 그대로 받지 않는다.
    if b.sense_ids.len() > 200 {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "한 번에 200장까지",
        )));
    }

    let Ok(deck) = uuid::Uuid::parse_str(&b.deck_id) else {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "덱을 고르지 못했다",
        )));
    };
    let sense_ids: Result<Vec<_>, _> = b
        .sense_ids
        .iter()
        .map(|s| s.parse::<uuid::Uuid>())
        .collect();
    let Ok(sense_ids) = sense_ids else {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected(
            "Sense 를 고르지 못했다",
        )));
    };
    let sense_ids: Vec<voca_store::SenseId> = sense_ids.into_iter().map(Into::into).collect();

    let cards = state
        .store()
        .add_cards(user.user_id, deck.into(), &sense_ids)
        .await
        .map_err(ApiError::store)?;

    Ok((
        axum::http::StatusCode::CREATED,
        axum::Json(
            cards
                .iter()
                .map(|c| CardBody {
                    card_id: c.id.to_string(),
                    deck_id: c.deck_id.to_string(),
                    sense_id: c.sense_id.to_string(),
                })
                .collect::<Vec<_>>(),
        ),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct UpdateDeckBody {
    pub deck_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub daily_goal: Option<u32>,
    #[serde(default)]
    pub new_per_day: Option<u32>,
}

pub async fn update_deck(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::Json<UpdateDeckBody>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    state.require_same_origin(&headers)?;

    let b = body.0;
    let Ok(deck_id) = uuid::Uuid::parse_str(&b.deck_id) else {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected("덱을 고르지 못했다")));
    };

    let deck = state
        .store()
        .update_deck(
            user.user_id,
            voca_store::DeckUpdate {
                id: deck_id.into(),
                name: b.name,
                description: None,
                daily_goal: b.daily_goal,
                new_per_day: b.new_per_day,
            },
        )
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(DeckBody {
        deck_id: deck.id.to_string(),
        name: deck.name,
        daily_goal: deck.daily_goal,
        new_per_day: deck.new_per_day,
    })
    .into_response())
}

pub async fn list_cards(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;

    let cards = state
        .store()
        .list_cards(user.user_id, None)
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(cards).into_response())
}

pub async fn delete_card(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(card_id): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    state.require_same_origin(&headers)?;

    let Ok(id) = uuid::Uuid::parse_str(&card_id) else {
        return Err(ApiError::from(voca_store::AuthFailure::Rejected("카드를 고르지 못했다")));
    };

    state
        .store()
        .archive_card(user.user_id, id.into())
        .await
        .map_err(ApiError::store)?;

    Ok(axum::Json(serde_json::json!({ "status": "deleted" })).into_response())
}


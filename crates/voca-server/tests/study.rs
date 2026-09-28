//! 복습 큐 · Review · 대시보드 라우트.
//!
//! 라우터가 **배선만** 하는지 확인한다. 카드를 DB 에 직접 밀어 넣지 않고
//! `Store` 의 공개 경로(`put_user_sense` → `create_deck` → `add_cards`)로 만든다 —
//! 화면이 실제로 거치는 길과 같아야 하기 때문이다.

mod common;

use common::{Harness, ORIGIN, registration};

/// 응답 본문에서 `user_id` 를 꺼낸다.
///
/// `Id` 는 `FromStr` 가 없다 — UUID 로 먼저 읽고 감싼다. 문자열로 눈치채고 조용히
/// 다른 값이 되어서는 안 된다.
fn user_id(body: &str) -> voca_domain::Id {
    let raw = serde_json::from_str::<serde_json::Value>(body).unwrap()["user_id"]
        .as_str()
        .expect("user_id 가 없다")
        .to_owned();
    voca_domain::Id::from(
        uuid::Uuid::parse_str(&raw)
            .unwrap_or_else(|e| panic!("user_id 가 UUID 가 아니다: {raw} ({e})")),
    )
}
use voca_domain::SenseKind;
use voca_store::{NewDeck, NewUserSense, Store};

/// 로그인까지 끝내고, 복습할 카드를 `count` 장 만든다.
async fn harness_with_cards(count: usize) -> (Harness, String, Vec<String>) {
    let h = Harness::start().await;
    let registered = h
        .post("/api/auth/register", Some(ORIGIN), registration("s@b.kr"))
        .await;
    assert_eq!(registered.status, 200, "{}", registered.body);
    let cookie = registered.cookie_header().expect("쿠키 없음");
    let user = user_id(&registered.body);

    let deck = h
        .store
        .create_deck(user, NewDeck::named("토익 3000"))
        .await
        .unwrap();

    let mut card_ids = Vec::new();
    for i in 0..count {
        let sense = h
            .store
            .put_user_sense(
                user,
                NewUserSense {
                    lemma: format!("단어{i}"),
                    kind: SenseKind::Word,
                    example_en: None,
                    definition: format!("뜻{i}"),
                    pos: Some("noun".into()),
                },
            )
            .await
            .unwrap();
        let cards = h.store.add_cards(user, deck.id, &[sense.id]).await.unwrap();
        card_ids.push(cards[0].id.to_string());
    }

    (h, cookie, card_ids)
}

#[tokio::test]
async fn the_study_queue_needs_a_login() {
    let (h, _cookie, _cards) = harness_with_cards(1).await;
    let reply = h.get("/api/study/queue", None).await;
    assert_eq!(reply.status, 401, "{}", reply.body);
}

#[tokio::test]
async fn the_queue_shows_the_front_the_sense_kind_asks_for() {
    let (h, cookie, _cards) = harness_with_cards(1).await;

    let reply = h.get("/api/study/queue", Some(&cookie)).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body = reply.json();

    assert_eq!(body["cards"].as_array().unwrap().len(), 1);
    let card = &body["cards"][0];
    // `word` Sense 의 앞면은 그 Word 의 표기형이다.
    assert_eq!(card["front"]["kind"], "lemma");
    assert_eq!(card["front"]["text"], "단어0");
    // 뒷면은 뜻풀이.
    assert_eq!(card["definition"], "뜻0");
    assert_eq!(card["pos"], "noun");
    assert_eq!(card["is_new"], true);
}

#[tokio::test]
async fn an_example_sense_shows_its_sentence_not_the_word() {
    let h = Harness::start().await;
    let registered = h
        .post("/api/auth/register", Some(ORIGIN), registration("e@b.kr"))
        .await;
    let cookie = registered.cookie_header().unwrap();
    let user = user_id(&registered.body);

    let sense = h
        .store
        .put_user_sense(
            user,
            NewUserSense {
                lemma: "run".to_owned(),
                kind: SenseKind::Example,
                example_en: Some("She runs every morning.".into()),
                definition: "매일 아침 달린다".into(),
                pos: None,
            },
        )
        .await
        .unwrap();
    let deck = h
        .store
        .create_deck(user, NewDeck::named("예문"))
        .await
        .unwrap();
    h.store.add_cards(user, deck.id, &[sense.id]).await.unwrap();

    let body = h.get("/api/study/queue", Some(&cookie)).await.json();
    // **표기형이 아니라 예문**을 보여줘야 한다 — 그게 예문 Sense 의 Card Front 다.
    assert_eq!(body["cards"][0]["front"]["kind"], "example");
    assert_eq!(body["cards"][0]["front"]["text"], "She runs every morning.");
    // 뜻은 여전히 뒷면에 있다.
    assert_eq!(body["cards"][0]["definition"], "매일 아침 달린다");
}

#[tokio::test]
async fn the_queue_limit_is_capped_by_the_server() {
    // 클라이언트가 10000 을 보내도 그대로 따르지 않는다.
    let (h, cookie, _cards) = harness_with_cards(3).await;

    let body = h
        .get("/api/study/queue?limit=10000", Some(&cookie))
        .await
        .json();
    assert_eq!(body["cards"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_review_updates_the_schedule_and_returns_the_four_buttons() {
    let (h, cookie, cards) = harness_with_cards(1).await;

    let reply = h
        .post_with_cookie(
            "/api/study/review",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "card_id": cards[0], "rating": "good" }),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body = reply.json();

    // 4개 Rating 모두 라벨이 있다 — 하나라도 비면 그 버튼이 안 눌린다.
    for rating in ["again", "hard", "good", "easy"] {
        let label = &body["preview"][rating]["label"];
        assert!(label.is_string(), "{rating} 라벨이 없다: {}", reply.body);
        assert!(!label.as_str().unwrap().is_empty());
    }
    // 다음 복습 시각이 앞으로 갔다.
    assert!(body["scheduled_days"].as_f64().unwrap() > 0.0);
    assert!(body["due_at"].is_string());
    // Good 은 XP 를 준다.
    assert_eq!(body["xp_earned"], 2);
    assert_eq!(body["streak"]["reviewed_today"], true);
}

#[tokio::test]
async fn again_costs_no_xp_but_keeps_the_streak_alive() {
    // Streak 은 Rating 이 아니라 **Review 가 있었다는 사실**로 이어진다.
    // 실패한 날을 끊으면 다시 시작해야 하는 셈이다.
    let (h, cookie, cards) = harness_with_cards(1).await;

    let reply = h
        .post_with_cookie(
            "/api/study/review",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "card_id": cards[0], "rating": "again" }),
        )
        .await;
    let body = reply.json();

    assert_eq!(body["xp_earned"], 0, "실패했는데 XP 를 줬다");
    assert_eq!(body["streak"]["current"], 1, "실패한 날이 Streak 를 끊는다");
    assert_eq!(body["streak"]["reviewed_today"], true);
}

#[tokio::test]
async fn the_review_removes_the_card_from_the_queue() {
    let (h, cookie, cards) = harness_with_cards(1).await;

    h.post_with_cookie(
        "/api/study/review",
        Some(ORIGIN),
        Some(&cookie),
        serde_json::json!({ "card_id": cards[0], "rating": "good" }),
    )
    .await;

    let body = h.get("/api/study/queue", Some(&cookie)).await.json();
    assert!(
        body["cards"].as_array().unwrap().is_empty(),
        "복습한 카드가 큐에 그대로 있다"
    );
}

#[tokio::test]
async fn a_review_from_a_foreign_origin_is_refused() {
    let (h, cookie, cards) = harness_with_cards(1).await;

    let reply = h
        .post_with_cookie(
            "/api/study/review",
            Some("https://evil.example.com"),
            Some(&cookie),
            serde_json::json!({ "card_id": cards[0], "rating": "good" }),
        )
        .await;
    // CSRF 거부는 400 이다 — 요청이 형식부터 틀렸다는 뜻. 403 과 구분하지 않는다.
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn a_review_needs_a_login() {
    let (h, _cookie, cards) = harness_with_cards(1).await;
    let reply = h
        .post("/api/study/review", Some(ORIGIN), {
            serde_json::json!({ "card_id": cards[0], "rating": "good" })
        })
        .await;
    assert_eq!(reply.status, 401, "{}", reply.body);
}

#[tokio::test]
async fn a_malformed_card_id_is_a_bad_request_not_a_server_error() {
    let (h, cookie, _cards) = harness_with_cards(1).await;
    let reply = h
        .post_with_cookie(
            "/api/study/review",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "card_id": "not-a-uuid", "rating": "good" }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn the_dashboard_counts_what_is_still_to_do() {
    let (h, cookie, _cards) = harness_with_cards(2).await;

    let reply = h.get("/api/dashboard", Some(&cookie)).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body = reply.json();

    assert_eq!(body["reviews_due"], 0, "아직 복습 전이다");
    assert_eq!(body["new_remaining"], 2);
    // 할 게 남았는데 오늘 안 했다 — 끊기기 직전.
    assert_eq!(body["streak_at_risk"], true);
    assert_eq!(body["decks"][0]["total"], 2);
    assert_eq!(body["decks"][0]["progress_percent"], 0);
}

#[tokio::test]
async fn the_dashboard_stops_flagging_risk_after_a_review() {
    let (h, cookie, cards) = harness_with_cards(2).await;

    h.post_with_cookie(
        "/api/study/review",
        Some(ORIGIN),
        Some(&cookie),
        serde_json::json!({ "card_id": cards[0], "rating": "good" }),
    )
    .await;

    let body = h.get("/api/dashboard", Some(&cookie)).await.json();
    assert_eq!(
        body["streak_at_risk"], false,
        "복습했는데 위험 표시가 남는다"
    );
    assert_eq!(body["streak"]["current"], 1);
    assert_eq!(body["level"]["level"], 1);
    assert_eq!(body["new_remaining"], 1);
    // 한 장을 복습했으니 진도가 움직인다.
    assert_eq!(body["decks"][0]["seen"], 1);
}

#[tokio::test]
async fn the_dashboard_needs_a_login() {
    let (h, _cookie, _cards) = harness_with_cards(1).await;
    let reply = h.get("/api/dashboard", None).await;
    assert_eq!(reply.status, 401, "{}", reply.body);
}

#[tokio::test]
async fn a_malformed_deck_param_is_a_bad_request_not_all_decks() {
    // 깨진 deck id를 `None`으로 바꾸면 "전체 덱"이 되어 고르지도 않은 카드가
    // 나온다. 조용한 오답이 조용한 실패보다 나쁘다.
    let (h, cookie, _cards) = harness_with_cards(1).await;
    let reply = h
        .get("/api/study/queue?deck=not-a-uuid", Some(&cookie))
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn a_dead_store_on_an_authenticated_route_reports_503_not_401() {
    // 저장소가 죽었는데 "비밀번호가 맞지 않습니다"(401)를 주면 사용자는
    // 비밀번호를 계속 바꾼다. 로그인과 같은 규칙이다 — 원인을 구분해 503을 준다.
    let (h, cookie, _cards) = harness_with_cards(1).await;
    h.store.pool().close().await;

    let reply = h.get("/api/study/queue", Some(&cookie)).await;
    assert_eq!(reply.status, 503, "{}", reply.body);
    assert_eq!(reply.code(), "server_down");
}

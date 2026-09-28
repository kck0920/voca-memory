//! 단어와 덱을 다루는 라우트.
//!
//! 선언대로 **옮겨 담아도 되는 경로**여야 한다. 여기서 하는 일은 검증과 배선뿐이다.

mod common;

use common::{Harness, ORIGIN, registration};
use voca_store::Store;

async fn registered() -> (Harness, String) {
    let h = Harness::start().await;
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("l@b.kr"))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    (h, reply.cookie_header().expect("쿠키 없음"))
}

#[tokio::test]
async fn every_library_route_needs_a_login() {
    let (h, _cookie) = registered().await;

    for (method, path, body) in [
        (
            "POST",
            "/api/senses",
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "달리다" }),
        ),
        ("GET", "/api/decks", serde_json::json!({})),
        ("POST", "/api/decks", serde_json::json!({ "name": "토익" })),
        (
            "POST",
            "/api/decks/cards",
            serde_json::json!({ "deck_id": "x", "sense_ids": ["y"] }),
        ),
    ] {
        let reply = match method {
            "GET" => h.get(path, None).await,
            _ => h.post(path, Some(ORIGIN), body).await,
        };
        assert_eq!(reply.status, 401, "{method} {path} 를 로그인 없이 열었다");
    }
}

#[tokio::test]
async fn a_blank_lemma_is_refused_by_the_domain_not_by_the_route() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "lemma": "   ", "kind": "word", "definition": "뜻" }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn an_example_sense_without_its_sentence_is_refused() {
    let (h, cookie) = registered().await;
    // 예문 Sense 의 Card Front 는 예문이다. 없으면 앞면을 그릴 수 없다.
    let reply = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "lemma": "run", "kind": "example", "definition": "달리다" }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn adding_a_sense_gives_back_ids_and_not_a_lemma_it_does_not_know() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({
                "lemma": "run", "kind": "word", "definition": "달리다", "pos": "verb"
            }),
        )
        .await;
    assert_eq!(reply.status, 201, "{}", reply.body);
    let body = reply.json();
    assert!(body["sense_id"].is_string());
    assert!(body["word_id"].is_string());
    assert_eq!(body["definition"], "달리다");
    assert_eq!(body["pos"], "verb");
    // **표기형이 없다.** 이 응답을 만들 때 안다는 뜻이 아니기 때문이다.
    assert!(
        body.get("lemma").is_none(),
        "모르는 값을 지어냈다: {}",
        reply.body
    );
}

#[tokio::test]
async fn a_new_deck_gets_the_documented_defaults() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/decks",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "name": "토익 3000" }),
        )
        .await;
    assert_eq!(reply.status, 201, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["name"], "토익 3000");
    assert_eq!(body["daily_goal"], 20, "하루 목표 기본값이 20 이어야 한다");
    assert_eq!(body["new_per_day"], 10);
    // 덱별 카드 수는 대시보드가 준다. 여기서 지어내지 않는다.
    assert!(body.get("card_count").is_none(), "{}", reply.body);
}

#[tokio::test]
async fn an_empty_deck_name_is_refused() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/decks",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "name": "  " }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn adding_cards_with_no_senses_is_refused() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/decks/cards",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "deck_id": "00000000-0000-0000-0000-000000000000", "sense_ids": [] }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn a_malformed_deck_id_is_a_bad_request() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/decks/cards",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "deck_id": "없는 덱", "sense_ids": ["아무거나"] }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

#[tokio::test]
async fn one_user_does_not_see_another_users_decks() {
    let (h, cookie_a) = registered().await;
    h.post_with_cookie(
        "/api/decks",
        Some(ORIGIN),
        Some(&cookie_a),
        serde_json::json!({ "name": "A의 덱" }),
    )
    .await;

    let b = h
        .post("/api/auth/register", Some(ORIGIN), registration("l2@b.kr"))
        .await
        .cookie_header()
        .unwrap();

    let mine = h.get("/api/decks", Some(&cookie_a)).await.json();
    let yours = h.get("/api/decks", Some(&b)).await.json();
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(yours.as_array().unwrap().len(), 0, "남의 덱이 보인다");
}

#[tokio::test]
async fn a_full_round_trip_lands_the_card_in_the_queue() {
    let (h, cookie) = registered().await;

    let deck = h
        .post_with_cookie(
            "/api/decks",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "name": "토익" }),
        )
        .await
        .json();
    let sense = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "달리다" }),
        )
        .await
        .json();

    let added = h
        .post_with_cookie(
            "/api/decks/cards",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({
                "deck_id": deck["deck_id"],
                "sense_ids": [sense["sense_id"]],
            }),
        )
        .await;
    assert_eq!(added.status, 201, "{}", added.body);
    assert_eq!(added.json().as_array().unwrap().len(), 1);

    let queue = h.get("/api/study/queue", Some(&cookie)).await.json();
    let card = &queue["cards"][0];
    assert_eq!(card["front"]["kind"], "lemma");
    assert_eq!(card["front"]["text"], "run");
    assert_eq!(card["definition"], "달리다");
}

#[tokio::test]
async fn the_same_sense_is_not_added_to_a_deck_twice() {
    let (h, cookie) = registered().await;
    let deck = h
        .post_with_cookie(
            "/api/decks",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "name": "토익" }),
        )
        .await
        .json();
    let sense = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "달리다" }),
        )
        .await
        .json();
    let payload = serde_json::json!({
        "deck_id": deck["deck_id"],
        "sense_ids": [sense["sense_id"]],
    });

    let first = h
        .post_with_cookie(
            "/api/decks/cards",
            Some(ORIGIN),
            Some(&cookie),
            payload.clone(),
        )
        .await;
    assert_eq!(first.json().as_array().unwrap().len(), 1);

    // 같은 Sense 를 두 번 넣어도 Card 는 하나다. 두 개면 복습이 두 번 나온다.
    let second = h
        .post_with_cookie("/api/decks/cards", Some(ORIGIN), Some(&cookie), payload)
        .await;
    assert_eq!(
        second.json().as_array().unwrap().len(),
        0,
        "같은 Sense 가 두 번 들어갔다: {}",
        second.body
    );
}

#[tokio::test]
async fn a_mutation_from_a_foreign_origin_is_refused() {
    let (h, cookie) = registered().await;
    let reply = h
        .post_with_cookie(
            "/api/decks",
            Some("https://evil.example.com"),
            Some(&cookie),
            serde_json::json!({ "name": "해킹" }),
        )
        .await;
    assert_eq!(reply.status, 400, "{}", reply.body);
}

// ── 동기화 ──────────────────────────────────────────────

#[tokio::test]
async fn sync_needs_a_login() {
    let (h, _cookie) = registered().await;
    let reply = h.get("/api/sync/changes", None).await;
    assert_eq!(reply.status, 401, "{}", reply.body);
}

#[tokio::test]
async fn an_empty_timeline_answers_honestly_rather_than_inventing_a_watermark() {
    let (h, cookie) = registered().await;

    // 아직 한 번도 바뀐 것이 없다. **0 이 정답이다.** 0 이 아니면 클라이언트가
    // "revision 0 이후" 를 "revision 3 이후" 로 바꿔 쓰면서 저장을 건너뛴다.
    let fresh = h.get("/api/sync/changes", Some(&cookie)).await;
    assert_eq!(fresh.status, 200, "{}", fresh.body);
    assert_eq!(fresh.json()["watermark"], 0);
    assert!(fresh.json()["changes"].as_array().unwrap().is_empty());

    // 무언가를 만들고 나면 watermark 가 올라간다.
    h.post_with_cookie(
        "/api/decks",
        Some(ORIGIN),
        Some(&cookie),
        serde_json::json!({ "name": "토익" }),
    )
    .await;

    let after = h
        .get("/api/sync/changes?since=0", Some(&cookie))
        .await
        .json();
    let watermark = after["watermark"].as_u64().unwrap();
    assert!(watermark > 0, "바뀐 것을 받았는데 watermark 가 0 이다");

    // 그 뒤로 아무것도 안 바뀌면, **빈 페이지여도 watermark 는 그대로 유지된다.**
    // 클라이언트가 이 값을 저장해야 다음에 놓치지 않는다.
    let quiet = h
        .get(
            &format!("/api/sync/changes?since={watermark}"),
            Some(&cookie),
        )
        .await
        .json();
    assert!(quiet["changes"].as_array().unwrap().is_empty());
    assert_eq!(quiet["watermark"].as_u64().unwrap(), watermark);
    assert_eq!(quiet["has_more"], false);
}

#[tokio::test]
async fn a_review_shows_up_in_the_change_timeline_with_its_memory_state() {
    // **Card 본문과 스케줄러 상태가 함께 와야 한다.** 빠지면 기기를 바꿨을 때 복습
    // 일정 전체가 뒤집힌다.
    let (h, cookie) = registered().await;
    let deck = h
        .post_with_cookie(
            "/api/decks",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "name": "토익" }),
        )
        .await
        .json();
    let sense = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "달리다" }),
        )
        .await
        .json();
    let card = h
        .post_with_cookie(
            "/api/decks/cards",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "deck_id": deck["deck_id"], "sense_ids": [sense["sense_id"]] }),
        )
        .await
        .json()[0]["card_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let queue = h.get("/api/study/queue", Some(&cookie)).await.json();
    let queued = queue["cards"][0]["card_id"].as_str().unwrap();
    h.post_with_cookie(
        "/api/study/review",
        Some(ORIGIN),
        Some(&cookie),
        serde_json::json!({ "card_id": queued, "rating": "good" }),
    )
    .await;

    let page = h
        .get("/api/sync/changes?since=0", Some(&cookie))
        .await
        .json();
    let card_change = page["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "card" && c["item"]["id"] == card)
        .unwrap_or_else(|| panic!("Card 변경이 타임라인에 없다: {page}"));

    let memory = &card_change["item"]["memory"];
    assert_eq!(memory["state"], "review", "복습했는데 new 로 남아 있다");
    assert!(
        memory["stability"].as_f64().unwrap() > 0.0,
        "안정성이 0 이다"
    );
    assert!(memory["due_at"].as_i64().unwrap() > 0);
    assert_eq!(memory["reps"], 1);
}

#[tokio::test]
async fn a_new_card_carries_no_stability_rather_than_zero() {
    // 0 과 비어 있음은 다른 뜻이다. 0 을 보내면 클라이언트가 "안정성 0"인
    // 성숙한 카드로 계산해 버린다.
    let (h, cookie) = registered().await;
    let deck = h
        .post_with_cookie(
            "/api/decks",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "name": "토익" }),
        )
        .await
        .json();
    let sense = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "달리다" }),
        )
        .await
        .json();
    h.post_with_cookie(
        "/api/decks/cards",
        Some(ORIGIN),
        Some(&cookie),
        serde_json::json!({ "deck_id": deck["deck_id"], "sense_ids": [sense["sense_id"]] }),
    )
    .await;

    let page = h
        .get("/api/sync/changes?since=0", Some(&cookie))
        .await
        .json();
    // 첫 변경이 덱일 수 있다. card 를 찾아본다.
    let card_change = page["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "card")
        .unwrap_or_else(|| panic!("Card 변경이 타임라인에 없다: {page}"));
    let memory = &card_change["item"]["memory"];
    assert_eq!(memory["state"], "new");
    assert!(
        memory["stability"].is_null(),
        "아직 복습 안 했는데 안정성이 {} 이다",
        memory["stability"]
    );
}

#[tokio::test]
async fn one_user_never_receives_another_users_changes() {
    let (h, cookie_a) = registered().await;
    h.post_with_cookie(
        "/api/decks",
        Some(ORIGIN),
        Some(&cookie_a),
        serde_json::json!({ "name": "A" }),
    )
    .await;

    let b = h
        .post("/api/auth/register", Some(ORIGIN), registration("l3@b.kr"))
        .await
        .cookie_header()
        .unwrap();
    let theirs = h.get("/api/sync/changes?since=0", Some(&b)).await.json();
    let names: Vec<&str> = theirs["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["item"]["name"].as_str())
        .collect();
    assert!(!names.contains(&"A"), "남의 덱 이름이 왔다: {names:?}");
}

#[tokio::test]
async fn the_sync_limit_is_capped_by_the_server() {
    let (h, cookie) = registered().await;
    let reply = h.get("/api/sync/changes?limit=99999", Some(&cookie)).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(reply.json()["changes"].as_array().unwrap().len() <= 1_000);
}

/// 두 계정이 같은 표기형에 자기 뜻을 만들어도 **서로의 뜻이 섞이지 않는다.**
///
/// `words` 에 `user_id` 가 없고 표기형만으로 사용자 Word를 찾던 때는, 두 사람이
/// 한 Word 를 공유해 A의 개인 뜻이 B의 조회에 그대로 나왔다.
#[tokio::test]
async fn two_accounts_writing_the_same_lemma_do_not_see_each_other() {
    let (h, cookie_a) = registered().await;
    let registered_a = h.get("/api/auth/me", Some(&cookie_a)).await.json()["user_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let user_a = voca_domain::Id::from(
        uuid::Uuid::parse_str(&registered_a).expect("user_id 가 UUID 가 아니다"),
    );

    let cookie_b = h
        .post("/api/auth/register", Some(ORIGIN), registration("l5@b.kr"))
        .await
        .cookie_header()
        .unwrap();

    // A와 B가 같은 표기형에 자기 뜻을 만든다.
    let a_sense = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie_a),
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "A만 보는 뜻" }),
        )
        .await
        .json();
    let b_sense = h
        .post_with_cookie(
            "/api/senses",
            Some(ORIGIN),
            Some(&cookie_b),
            serde_json::json!({ "lemma": "run", "kind": "word", "definition": "B만 보는 뜻" }),
        )
        .await
        .json();

    assert_ne!(
        a_sense["word_id"], b_sense["word_id"],
        "두 계정이 한 Word 를 공유한다 — 서로의 개인 뜻이 새어 나간다"
    );

    // A의 조회에 B의 뜻이 없다. (반대 방향도 같은 질의로 보장된다.)
    let a_view = h
        .store
        .find_word(user_a, "run")
        .await
        .unwrap()
        .expect("A의 단어가 없다");
    assert!(
        !a_view.senses.iter().any(|s| s.definition == "B만 보는 뜻"),
        "A의 조회에 B의 뜻이 나왔다: {:?}",
        a_view
            .senses
            .iter()
            .map(|s| &s.definition)
            .collect::<Vec<_>>()
    );
}

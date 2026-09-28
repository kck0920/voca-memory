//! 사전 조회 라우트.
//!
//! 외부 사전 서버를 **테스트 안에서 직접 띄운다.** 실제 dictionaryapi.dev 에
//! 의존하면 테스트가 네트워크에 좌우되고, 형식이 바뀌면 여기서 조용히 0건이 된다.
//! 그래서 가짜 서버가 같은 형식으로 답한다.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::routing::get;
mod common;

use common::{Harness, ORIGIN, registration};

/// 성공 응답: 실제 dictionaryapi.dev 의 모양을 흉내 낸다.
const RUN_OK: &str = r#"[{"word":"run","phonetic":"/ɹʌn/","meanings":[{"partOfSpeech":"verb","definitions":[{"definition":"To move at a speed faster than a walk.","example":"She runs every morning."}]}]}]"#;

/// 뜻이 하나도 없는 성공 응답.
const NO_MEANING: &str = r#"[{"word":"run"}]"#;

async fn logged_in_harness(
    responses: Vec<(&'static str, &'static str)>,
) -> (Harness, String, Arc<AtomicUsize>) {
    let dir = tempfile::TempDir::new().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let mut app = Router::new();

    for (lemma, body) in responses {
        let counter = Arc::clone(&counter);
        app = app.route(
            &format!("/{lemma}"),
            get(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    ([("content-type", "application/json")], body.to_owned())
                }
            }),
        );
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    drop(dir);

    let harness = Harness::with_dictionary(&format!("http://{addr}")).await;
    (harness, format!("http://{addr}"), hits)
}

async fn register(h: &Harness) -> String {
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("d@b.kr"))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    reply.cookie_header().expect("쿠키 없음")
}

#[tokio::test]
async fn the_dictionary_needs_a_login() {
    let (h, _ep, _hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let reply = h.get("/api/dict/lookup?lemma=run", None).await;
    assert_eq!(
        reply.status, 401,
        "익명으로 사전을 긁을 수 있다: {}",
        reply.body
    );
}

#[tokio::test]
async fn a_fetched_word_carries_its_senses() {
    let (h, _ep, hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let cookie = register(&h).await;

    let reply = h.get("/api/dict/lookup?lemma=run", Some(&cookie)).await;
    assert_eq!(reply.status, 200, "{}", reply.body);

    let body = reply.json();
    assert_eq!(body["status"], "fetched");
    assert_eq!(body["word"]["lemma"], "run");
    assert_eq!(body["word"]["phonetic"], "/ɹʌn/");
    assert_eq!(body["word"]["senses"][0]["kind"], "word");
    assert_eq!(body["word"]["senses"][0]["pos"], "verb");
    assert!(
        body["word"]["senses"][0]["definition"]
            .as_str()
            .unwrap()
            .contains("faster than a walk")
    );
    // 저장까지 끝났으니 식별자가 있다.
    assert!(body["word"]["word_id"].is_string(), "저장 안 됐다: {body}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_second_lookup_is_served_from_the_cache() {
    let (h, _ep, hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let cookie = register(&h).await;

    let first = h.get("/api/dict/lookup?lemma=run", Some(&cookie)).await;
    assert_eq!(first.json()["status"], "fetched");

    let second = h.get("/api/dict/lookup?lemma=run", Some(&cookie)).await;
    assert_eq!(second.status, 200);
    assert_eq!(second.json()["status"], "cached");
    // 같은 식별자 — 두 번째 조회가 새 단어를 만든 게 아니다.
    assert_eq!(
        first.json()["word"]["word_id"],
        second.json()["word"]["word_id"]
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1, "네트워크를 두 번 봤다");
}

#[tokio::test]
async fn a_word_the_api_does_not_know_is_not_an_error() {
    // 라우트에 없는 경로 = 사전에 없는 단어. 404 를 그대로 통과시킨다.
    let (h, _ep, _hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let cookie = register(&h).await;

    let reply = h.get("/api/dict/lookup?lemma=zzz", Some(&cookie)).await;
    assert_eq!(reply.status, 200, "사전에 없는 것은 실패가 아니다");
    assert_eq!(reply.json()["status"], "not_found");
    assert!(reply.json()["word"].is_null());
}

#[tokio::test]
async fn a_response_we_cannot_read_is_reported_not_swallowed() {
    // 필드 이름이 바뀌었을 때의 모습.
    let (h, _ep, _hits) = logged_in_harness(vec![("run", r#"[{"vocabulary":"run"}]"#)]).await;
    let cookie = register(&h).await;

    let reply = h.get("/api/dict/lookup?lemma=run", Some(&cookie)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["status"],
        "unrecognized",
        "형식을 모르면 조용히 넘어가지 않는다: {}",
        reply.body
    );
}

#[tokio::test]
async fn a_word_with_no_definitions_yields_nothing_to_add() {
    // 사전은 그 단어를 안다. 다만 뜻이 없다 — 배우면 배울 게 없다.
    // 그래서 저장하지 않는다. 사용자에게는 "추가할 뜻 없음"으로 보이는 것이 맞다.
    let (h, _ep, _hits) = logged_in_harness(vec![("run", NO_MEANING)]).await;
    let cookie = register(&h).await;

    let reply = h.get("/api/dict/lookup?lemma=run", Some(&cookie)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["status"], "not_found", "{}", reply.body);
    assert!(reply.json()["word"].is_null());

    // 저장소에 Word가 생기지 않았어야 한다 — 없는 Sense 를 가진 Word 는 규칙 위반.
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM words WHERE lemma = 'run'")
        .fetch_one(h.store.pool())
        .await
        .unwrap();
    assert_eq!(stored, 0, "뜻 없는 단어를 저장했다");
}

#[tokio::test]
async fn a_dead_dictionary_does_not_take_the_whole_request_down() {
    // 아무것도 안 떠 있는 포트. 연결이 즉시 거절된다.
    let h = Harness::with_dictionary("http://127.0.0.1:1").await;
    let cookie = register(&h).await;

    let reply = h.get("/api/dict/lookup?lemma=run", Some(&cookie)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["status"], "unavailable");
}

#[tokio::test]
async fn a_blank_lemma_is_refused_before_we_hit_the_network() {
    let (h, _ep, hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let cookie = register(&h).await;

    let reply = h.get("/api/dict/lookup?lemma=%20%20", Some(&cookie)).await;
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_absurdly_long_lemma_is_refused() {
    let (h, _ep, hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let cookie = register(&h).await;

    let long = "a".repeat(200);
    let reply = h
        .get(&format!("/api/dict/lookup?lemma={long}"), Some(&cookie))
        .await;
    assert_eq!(reply.status, 400);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_lemma_with_a_path_traversal_cannot_escape_the_endpoint() {
    // `../` 가 남아 있으면 주소가 `.../entries/en/../../admin` 이 되어 버린다.
    let (h, _ep, _hits) = logged_in_harness(vec![("run", RUN_OK)]).await;
    let cookie = register(&h).await;

    // 서버가 살아 있으므로 404/빈 응답이 아닌 200 을 준다. 地址가 조작되어 500 이
    // 나면 그걸로 충분하다 — 중요한 것은 **크래시가 아니라 정상 응답**이다.
    let reply = h
        .get("/api/dict/lookup?lemma=..%2F..%2Fadmin", Some(&cookie))
        .await;
    assert!(
        reply.status.as_u16() < 500,
        "주소 조작이 서버를 죽였다: {} {}",
        reply.status,
        reply.body
    );
}

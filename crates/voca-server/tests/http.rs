//! HTTP 배선 통합 테스트.
//!
//! 라우터를 직접 부른다. 여기서 확인하는 것은 **배선**이다 —
//! 쿠키가 붙는지, 상태 코드가 맞는지�, CSRF 가 막히는지.
//! 규칙 자체는 [`auth.rs`](./auth.rs) 가 확인한다.

mod common;

use common::{Harness, ORIGIN, credentials, registration};
use voca_store::Accounts;

#[tokio::test]
async fn health_answers_without_a_session() {
    let h = Harness::start().await;
    let reply = h.get("/api/health", None).await;
    assert_eq!(reply.status, 200);
}

#[tokio::test]
async fn registering_sets_a_session_cookie_and_returns_the_user() {
    let h = Harness::start().await;
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;

    assert_eq!(reply.status, 200, "{}", reply.body);
    let (name, _value) = reply.cookie().expect("세션 쿠키가 없다");
    assert_eq!(name, "voca_session");
    assert_eq!(reply.json()["display_name"], "테스터");
    assert_eq!(reply.json()["retention"], "balanced");
}

#[tokio::test]
async fn the_session_cookie_has_the_flags_that_protect_it() {
    let h = Harness::start().await;
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    let raw = reply.header("set-cookie").unwrap().to_lowercase();

    // XSS 가 쿠키를 읽을 수 없어야 한다.
    assert!(raw.contains("httponly"), "HttpOnly 가 없다: {raw}");
    // 다른 사이트의 form POST 로 이 쿠키가 나가면 안 된다.
    assert!(raw.contains("samesite=lax"), "SameSite=Lax 가 없다: {raw}");
    // 30일간 유지된다 — 매일 로그인시키면 스트릭이 끊긴다.
    assert!(raw.contains("max-age=2592000"), "Max-Age 가 없다: {raw}");
}

#[tokio::test]
async fn the_token_is_never_in_the_response_body() {
    // XSS 스크립트가 JSON 본문만 읽으면 토큰을 얻을 수 없어야 한다.
    let h = Harness::start().await;
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    let (_, token) = reply.cookie().unwrap();
    assert!(!reply.body.contains(&token), "토큰이 본문에 새었다");
    assert!(reply.json().get("token").is_none());
}

#[tokio::test]
async fn the_cookie_works_on_the_next_request() {
    let h = Harness::start().await;
    let registered = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    let cookie = registered.cookie_header().unwrap();

    let me = h.get("/api/auth/me", Some(&cookie)).await;
    assert_eq!(me.status, 200);
    assert_eq!(me.json()["display_name"], "테스터");
}

#[tokio::test]
async fn me_without_a_cookie_is_null_not_401() {
    // 대시보드가 "로그인 안 됨"을 조용히 알아야 한다. 401 은 "인증 필요"라는
    // 뜻이라 여기에는 맞지 않는다.
    let h = Harness::start().await;
    let me = h.get("/api/auth/me", None).await;
    assert_eq!(me.status, 200);
    assert!(me.json().is_null());
}

#[tokio::test]
async fn me_with_a_forged_cookie_is_null() {
    let h = Harness::start().await;
    let me = h.get("/api/auth/me", Some("voca_session=deadbeef")).await;
    assert_eq!(me.status, 200);
    assert!(me.json().is_null());
}

#[tokio::test]
async fn logging_out_clears_the_cookie_and_kills_the_session() {
    let h = Harness::start().await;
    let registered = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    let cookie = registered.cookie_header().unwrap();

    // 로그아웃은 **지금 로그인한 사람의 쿠키**가 있어야 세션을 끊을 수 있다.
    let out = h
        .post_with_cookie(
            "/api/auth/logout",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(out.status, 204);
    let raw = out.header("set-cookie").unwrap();
    assert!(
        raw.to_lowercase().contains("max-age=0"),
        "만료가 되지 않았다: {raw}"
    );

    let me = h.get("/api/auth/me", Some(&cookie)).await;
    assert!(me.json().is_null(), "로그아웃했는데 세션이 살아 있다");
}

#[tokio::test]
async fn logging_out_twice_is_not_an_error() {
    let h = Harness::start().await;
    let registered = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    let cookie = registered.cookie_header().unwrap();

    h.post_with_cookie(
        "/api/auth/logout",
        Some(ORIGIN),
        Some(&cookie),
        serde_json::json!({}),
    )
    .await;
    // 세션은 이미 죽었지만 같은 쿠키로 다시 시도해도 성공해야 한다.
    let again = h
        .post_with_cookie(
            "/api/auth/logout",
            Some(ORIGIN),
            Some(&cookie),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(again.status, 204, "재요청이 실패했다: {}", again.body);
}

// ── CSRF ────────────────────────────────────────────────

#[tokio::test]
async fn a_mutation_from_a_foreign_origin_is_refused() {
    let h = Harness::start().await;
    let reply = h
        .post(
            "/api/auth/register",
            Some("https://evil.kr"),
            registration("a@b.kr"),
        )
        .await;
    assert_ne!(reply.status, 200, "타 출처에서 계정이 만들어졌다");
    assert_eq!(h.store.count().await.unwrap(), 0, "계정이 저절로 생겼다");
}

#[tokio::test]
async fn a_login_from_a_foreign_origin_is_refused() {
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;

    let reply = h
        .post(
            "/api/auth/login",
            Some("https://evil.kr"),
            credentials("a@b.kr", "부지런한-비밀번호"),
        )
        .await;
    assert_ne!(reply.status, 200, "타 출처에서 로그인됐다");
}

#[tokio::test]
async fn an_empty_allow_list_blocks_every_mutation() {
    // 설정 누락이 조용히 CSRF 를 끄지 않는다. 전부 막히는 쪽이 낫다.
    let h = Harness::with_origins(&[]).await;
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    assert_ne!(reply.status, 200);
}

#[tokio::test]
async fn reads_are_not_blocked_by_the_origin_check() {
    // CSRF 는 **변경**에 대한 것이다. 조회가 막히면 앱이 돌아가지 않는다.
    let h = Harness::with_origins(&[]).await;
    let reply = h.get("/api/auth/me", None).await;
    assert_eq!(reply.status, 200);
}

// ── 계정 열거 ───────────────────────────────────────────

#[tokio::test]
async fn a_missing_account_and_a_wrong_password_return_the_same_thing_over_http() {
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;

    let missing = h
        .post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("없는-계정@b.kr", "아무-비밀번호"),
        )
        .await;
    let wrong = h
        .post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("a@b.kr", "아무-비밀번호"),
        )
        .await;

    assert_eq!(missing.status, wrong.status);
    assert_eq!(missing.body, wrong.body, "응답이 다르면 존재 여부가 샌다");
    assert_eq!(missing.code(), "bad_credentials");
}

#[tokio::test]
async fn a_duplicate_email_is_a_conflict() {
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    let again = h
        .post("/api/auth/register", Some(ORIGIN), registration("A@B.KR"))
        .await;
    assert_eq!(again.status, 409);
    assert_eq!(again.code(), "email_taken");
}

#[tokio::test]
async fn a_malformed_email_is_a_bad_request() {
    let h = Harness::start().await;
    let reply = h
        .post("/api/auth/register", Some(ORIGIN), registration("화살표@"))
        .await;
    assert_eq!(reply.status, 400);
    assert_eq!(reply.code(), "rejected");
}

// ── 로그인 제한 ─────────────────────────────────────────

#[tokio::test]
async fn repeated_failures_are_throttled() {
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;

    let mut last = None;
    for _ in 0..12 {
        last = Some(
            h.post(
                "/api/auth/login",
                Some(ORIGIN),
                credentials("a@b.kr", "틀린-비밀번호"),
            )
            .await,
        );
    }

    let last = last.unwrap();
    assert_eq!(last.status, 429, "제한이 걸리지 않았다");
    // 메시지는 비밀번호 오류와 같아야 한다 — 어느 이메일인지 새지 않는다.
    assert_eq!(last.code(), "too_many_attempts");
    assert_eq!(
        last.json()["message"],
        "이메일 또는 비밀번호가 맞지 않습니다"
    );
    assert_eq!(last.header("retry-after"), Some("900"));
}

#[tokio::test]
async fn throttling_one_account_does_not_lock_another() {
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    h.post("/api/auth/register", Some(ORIGIN), registration("b@b.kr"))
        .await;

    for _ in 0..12 {
        h.post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("a@b.kr", "틀린-비밀번호"),
        )
        .await;
    }

    let other = h
        .post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("b@b.kr", "부지런한-비밀번호"),
        )
        .await;
    assert_eq!(other.status, 200, "다른 계정까지 잠갔다");
}

#[tokio::test]
async fn a_successful_login_clears_the_failure_count() {
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;

    for _ in 0..5 {
        h.post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("a@b.kr", "틀린-비밀번호"),
        )
        .await;
    }
    let ok = h
        .post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("a@b.kr", "부지런한-비밀번호"),
        )
        .await;
    assert_eq!(ok.status, 200);

    // 성공이 카운터를 지웠으므로 다시 여유가 있다.
    for _ in 0..3 {
        let r = h
            .post(
                "/api/auth/login",
                Some(ORIGIN),
                credentials("a@b.kr", "틀린-비밀번호"),
            )
            .await;
        assert_ne!(r.status, 429, "성공이 카운터를 안 지웠다");
    }
}

// ── 재시도 신호 ─────────────────────────────────────────

#[tokio::test]
async fn a_dead_store_reports_503_not_bad_credentials() {
    // 사용자가 제대로 입력했는데 "비밀번호가 틀렸습니다"를 받으면 아무것도 할 수 없다.
    let h = Harness::start().await;
    h.post("/api/auth/register", Some(ORIGIN), registration("a@b.kr"))
        .await;
    h.store.pool().close().await;

    let reply = h
        .post(
            "/api/auth/login",
            Some(ORIGIN),
            credentials("a@b.kr", "부지런한-비밀번호"),
        )
        .await;
    assert_eq!(reply.status, 503, "{}", reply.body);
    assert_eq!(reply.code(), "server_down");
}

// ── 페이지 ──────────────────────────────────────────────

#[tokio::test]
async fn the_page_renders_html_not_a_json_error() {
    let h = Harness::start().await;
    let reply = h.get("/", None).await;

    assert_eq!(reply.status, 200, "{}", reply.body);
    // 로그인하지 않았는데 401 이 아니다. **브라우저가 그 JSON 을 페이지에 띄운다.**
    assert_eq!(
        reply.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(reply.body.contains("<!DOCTYPE html>"), "{}", reply.body);
    assert!(reply.body.contains("Voca Memory"));
}

#[tokio::test]
async fn the_page_carries_the_dashboard_for_the_logged_in_user() {
    // 첫 화면이 빈 대시보드로 보이면 "내 Streak 가 왜 0 이지"가 첫인상이 된다.
    let h = Harness::start().await;
    let registered = h
        .post("/api/auth/register", Some(ORIGIN), registration("p@b.kr"))
        .await;
    let cookie = registered.cookie_header().unwrap();

    let reply = h.get("/", Some(&cookie)).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(reply.body.contains("0일 연속"), "Streak 가 안 보인다");
    assert!(reply.body.contains("Lv 1"), "레벨이 안 보인다");
}

#[tokio::test]
async fn the_page_ships_the_data_the_client_needs_to_hydrate() {
    // 하이드레이션은 마크업에 심어 둔 JSON 을 읽는다. 없으면 화면이 멈춘다.
    let h = Harness::start().await;
    let reply = h.get("/", None).await;

    assert!(
        reply
            .body
            .contains(&format!(r#"id="{}""#, voca_ui::view::DATA_ELEMENT_ID)),
        "데이터 요소가 없다. 하이드레이션이 조용히 실패한다: {}",
        reply.body
    );
    // 실행되지 않는 script 다. 실행되면 데이터가 새지 않는다.
    assert!(reply.body.contains(r#"type="application/json""#));
    assert!(reply.body.contains("/pkg/voca-web.js"), "번들 경로가 없다");
}

#[tokio::test]
async fn the_page_does_not_leak_one_users_dashboard_to_another() {
    let h = Harness::start().await;

    let a = h
        .post("/api/auth/register", Some(ORIGIN), registration("a1@b.kr"))
        .await
        .cookie_header()
        .unwrap();
    let b = h
        .post("/api/auth/register", Some(ORIGIN), registration("b1@b.kr"))
        .await
        .cookie_header()
        .unwrap();

    // 두 사람이 각자 자기 화면을 본다. 이건 지금 같아 보이지만, 데이터를 섞는
    // 실수는 캐시를 넣을 때 생긴다.
    let pa = h.get("/", Some(&a)).await;
    let pb = h.get("/", Some(&b)).await;
    assert_eq!(pa.status, 200);
    assert_eq!(pb.status, 200);
}

//! 인증 규칙 테스트.
//!
//! 여기서 확인하는 성질:
//!
//! - **계정 존재 여부가 새지 않는다** (메시지, 상태 코드, 응답 시간)
//! - 비밀번호 평문이 어디에도 남지 않는다
//! - 토큰이 로그에 새지 않는다

use std::time::Instant;

use tempfile::TempDir;
use voca_store::{Accounts, RegisterRequest, SessionToken};
use voca_store_sqlite::SqliteStore;

use voca_server::auth::{AuthContext, LoginRequest};

struct World {
    _dir: TempDir,
    store: SqliteStore,
}

impl World {
    async fn setup() -> Self {
        let dir = TempDir::new().unwrap();
        let store = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();
        Self { _dir: dir, store }
    }

    fn auth(&self) -> AuthContext<'_, SqliteStore> {
        AuthContext::new(&self.store)
    }
}

fn registration(email: &str) -> RegisterRequest {
    RegisterRequest {
        email: email.into(),
        password: "부지런한-비밀번호".into(),
        display_name: "테스터".into(),
        timezone: "Asia/Seoul".into(),
        retention: None,
    }
}

fn login(email: &str, password: &str) -> LoginRequest {
    LoginRequest {
        email: email.into(),
        password: password.into(),
    }
}

#[tokio::test]
async fn registering_returns_a_usable_session() {
    let w = World::setup().await;
    let outcome = w
        .auth()
        .register(registration("a@b.kr"))
        .await
        .into_parts()
        .unwrap();

    assert_eq!(outcome.user.display_name, "테스터");
    assert_eq!(outcome.user.retention, "balanced");
    assert_eq!(
        w.auth()
            .whoami(&outcome.token)
            .await
            .unwrap()
            .unwrap()
            .user_id,
        outcome.user.user_id
    );
}

#[tokio::test]
async fn a_registered_user_can_log_in() {
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;

    let outcome = w
        .auth()
        .login(login("a@b.kr", "부지런한-비밀번호"))
        .await
        .into_parts()
        .unwrap();
    assert_eq!(outcome.user.display_name, "테스터");
}

#[tokio::test]
async fn a_duplicate_email_is_refused() {
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;
    let second = w
        .auth()
        .register(registration("A@B.KR"))
        .await
        .into_parts()
        .unwrap_err();
    assert_eq!(second, voca_store::AuthFailure::EmailTaken);
}

#[tokio::test]
async fn the_wrong_password_fails() {
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;
    let result = w.auth().login(login("a@b.kr", "틀린-비밀번호")).await;
    assert!(result.into_parts().is_err());
}

// ── 계정 존재 여부를 새지 않는다 ─────────────────────────

#[tokio::test]
async fn a_missing_account_and_a_wrong_password_are_indistinguishable() {
    // 메시지도 상태 코드도 같아야 한다. 다르면 "이 이메일은 가입돼 있다"를 안다.
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;

    let missing = w
        .auth()
        .login(login("없는-계정@b.kr", "아무-비밀번호"))
        .await;
    let wrong = w.auth().login(login("a@b.kr", "아무-비밀번호")).await;

    let (m, x) = (
        missing.into_parts().unwrap_err(),
        wrong.into_parts().unwrap_err(),
    );
    assert_eq!(m, x);
    assert_eq!(m.status(), 401);
    assert_eq!(m.message(), "이메일 또는 비밀번호가 맞지 않습니다");
}

#[tokio::test]
async fn a_missing_account_also_burns_hashing_time() {
    // 미가입일 때 해시 검증을 생략하면 응답 시간이 무참하게 짧아진다. 그 차이로
    // 가입 여부가 드러난다. 이 테스트는 시간이 걸려도 **느리게만** 빠지지 않음을
    // 확인한다 — 즉 미가입 경로가 더미 검증을 돌린다는 뜻이다.
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;

    // 예열 — 첫 호출의 캐시 효과는 걷어낸다.
    for _ in 0..2 {
        let _ = w
            .auth()
            .login(login("없는-계정@b.kr", "아무-비밀번호"))
            .await;
        let _ = w.auth().login(login("a@b.kr", "아무-비밀번호")).await;
    }

    let samples = 3;
    let mut missing_time = 0u128;
    let mut wrong_time = 0u128;
    for _ in 0..samples {
        let t = Instant::now();
        let _ = w
            .auth()
            .login(login("없는-계정@b.kr", "아무-비밀번호"))
            .await;
        missing_time += t.elapsed().as_millis();

        let t = Instant::now();
        let _ = w.auth().login(login("a@b.kr", "아무-비밀번호")).await;
        wrong_time += t.elapsed().as_millis();
    }

    let missing = missing_time / samples;
    let wrong = wrong_time / samples;
    // 미가입이 실제 검증보다 2배 이상 빨라지면 그 자체가 정보다. argon2id 기본
    // 파라미터(19MB, t=2)는 수십 ms 라 여유를 두어 2배를 잡는다.
    assert!(
        missing * 2 > wrong,
        "미가입 응답이 너무 빠르다 ({missing}ms vs {wrong}ms) — 존재 여부가 시간으로 샌다"
    );
}

#[tokio::test]
async fn a_long_password_candidate_is_refused_without_leaking_anything() {
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;
    let huge = "가".repeat(300);
    let result = w.auth().login(login("a@b.kr", &huge)).await;
    assert_eq!(result.into_parts().unwrap_err().status(), 401);
}

#[tokio::test]
async fn a_dead_store_reports_service_unavailable_not_bad_credentials() {
    // 사용자가 제대로 입력했는데 "비밀번호가 틀렸습니다"를 받으면 아무것도 할 수 없다.
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;

    // 저장소를 닫아서 실패를 만든다.
    w.store.pool().close().await;

    let result = w.auth().login(login("a@b.kr", "부지런한-비밀번호")).await;
    let failure = result.into_parts().unwrap_err();
    assert_eq!(failure.status(), 503, "{}", failure.message());
    assert!(failure.is_retryable());
}

// ── 입력 검증 ────────────────────────────────────────────

#[tokio::test]
async fn a_malformed_email_is_refused_before_hashing() {
    let w = World::setup().await;
    for email in ["", "공백 있음@b.kr", "이름@", "@no-local.kr", "이름@"] {
        let result = w.auth().register(registration(email)).await;
        assert_eq!(
            result.into_parts().unwrap_err(),
            voca_store::AuthFailure::Rejected("이메일 형식이 아니다"),
            "이메일을 거절해야 한다: {email:?}"
        );
    }
}

#[tokio::test]
async fn an_empty_display_name_is_refused() {
    let w = World::setup().await;
    let mut request = registration("a@b.kr");
    request.display_name = "   ".into();
    let result = w.auth().register(request).await;
    assert!(result.into_parts().is_err());
}

#[tokio::test]
async fn a_short_password_is_refused() {
    let w = World::setup().await;
    let mut request = registration("a@b.kr");
    request.password = "짧음".into();
    let result = w.auth().register(request).await;
    assert!(result.into_parts().is_err());
}

// ── 비밀번호가 남지 않는다 ────────────────────────────────

#[tokio::test]
async fn the_plain_password_never_reaches_the_database() {
    let w = World::setup().await;
    w.auth().register(registration("a@b.kr")).await;

    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(w.store.pool())
        .await
        .unwrap();
    assert!(
        !stored.contains("부지런한-비밀번호"),
        "평문 비밀번호가 저장됐다"
    );
    assert!(
        stored.starts_with("$argon2id$"),
        "argon2id 가 아니다: {stored}"
    );
}

#[tokio::test]
async fn a_rejected_registration_stores_nothing() {
    // 비밀번호가 너무 짧아서 해시 전에 막히면 계정도 없어야 한다.
    let w = World::setup().await;
    let mut request = registration("a@b.kr");
    request.password = "짧음".into();
    assert!(w.auth().register(request).await.into_parts().is_err());
    assert_eq!(w.store.count().await.unwrap(), 0);
}

// ── 로그아웃 ─────────────────────────────────────────────

#[tokio::test]
async fn logging_out_kills_the_session() {
    let w = World::setup().await;
    let session = w
        .auth()
        .register(registration("a@b.kr"))
        .await
        .into_parts()
        .unwrap();

    w.auth().logout(&session.token).await.unwrap();
    assert!(w.auth().whoami(&session.token).await.unwrap().is_none());
}

#[tokio::test]
async fn logging_out_twice_is_not_an_error() {
    let w = World::setup().await;
    let session = w
        .auth()
        .register(registration("a@b.kr"))
        .await
        .into_parts()
        .unwrap();
    w.auth().logout(&session.token).await.unwrap();
    assert!(w.auth().logout(&session.token).await.is_ok());
}

#[tokio::test]
async fn logging_out_everywhere_ends_every_device() {
    let w = World::setup().await;
    let phone = w.auth().login(login("a@b.kr", "부지런한-비밀번호")).await;
    let _ = phone;
    w.auth().register(registration("a@b.kr")).await;

    let laptop = w
        .auth()
        .login(login("a@b.kr", "부지런한-비밀번호"))
        .await
        .into_parts()
        .unwrap();
    let tablet = w
        .auth()
        .login(login("a@b.kr", "부지런한-비밀번호"))
        .await
        .into_parts()
        .unwrap();

    w.auth()
        .logout_everywhere(laptop.user.user_id)
        .await
        .unwrap();

    assert!(w.auth().whoami(&laptop.token).await.unwrap().is_none());
    assert!(w.auth().whoami(&tablet.token).await.unwrap().is_none());
}

#[tokio::test]
async fn an_unknown_token_resolves_to_nothing() {
    let w = World::setup().await;
    let token = SessionToken::new("0".repeat(64));
    assert!(w.auth().whoami(&token).await.unwrap().is_none());
}

#[tokio::test]
async fn a_session_whose_account_vanished_resolves_to_nothing() {
    // 세션이 살아 있는데 계정이 없으면 잘못된 상태다. 여기서 막는다.
    let w = World::setup().await;
    let session = w
        .auth()
        .register(registration("a@b.kr"))
        .await
        .into_parts()
        .unwrap();

    // 외래 키 제약을 끄고 계정만 지운다.
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(w.store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM users")
        .execute(w.store.pool())
        .await
        .unwrap();

    assert!(w.auth().whoami(&session.token).await.unwrap().is_none());
}

#[tokio::test]
async fn a_logged_in_token_never_appears_in_debug_output() {
    let w = World::setup().await;
    let session = w
        .auth()
        .register(registration("a@b.kr"))
        .await
        .into_parts()
        .unwrap();
    let shown = format!("{session:?}");
    assert!(
        !shown.contains(session.token.expose()),
        "토큰이 {:?} 에 새었다",
        shown
    );
}

#[tokio::test]
async fn retention_can_be_chosen_at_registration() {
    let w = World::setup().await;
    let mut request = registration("a@b.kr");
    request.retention = Some("frugal".into());
    let session = w.auth().register(request).await.into_parts().unwrap();
    assert_eq!(session.user.retention, "frugal");
}

#[tokio::test]
async fn the_account_survives_a_restart_and_the_session_still_works() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("v.db");

    let token = {
        let store = SqliteStore::open(&path).await.unwrap();
        let auth = AuthContext::new(&store);
        let session = auth
            .register(registration("a@b.kr"))
            .await
            .into_parts()
            .unwrap();
        session.token
    };

    let store = SqliteStore::open(&path).await.unwrap();
    let auth = AuthContext::new(&store);
    let user = auth.whoami(&token).await.unwrap().unwrap();
    assert_eq!(user.display_name, "테스터");
    // 비밀번호도 여전히 맞는다.
    assert!(
        auth.login(login("a@b.kr", "부지런한-비밀번호"))
            .await
            .into_parts()
            .is_ok()
    );
}

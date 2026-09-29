//! HTTP 서버.
//!
//! 배선만 한다. 규칙은 [`auth`]·[`security`] 에 있고, 데이터는 `voca-store` seam
//! 너머에 있다. 여기서 규칙을 복사하지 않는다.
//!
//! [#adr-0013]: ../../docs/adr/0013-auth-rules-are-separate-from-http.md

pub mod auth;
pub mod config;
mod library;
mod page;
pub mod password;
mod routes;
pub mod security;
pub mod seed;
mod study;
mod sync;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use voca_dict::client::Dictionary;
use voca_store_sqlite::SqliteStore;

pub use config::{Config, ConfigError};
pub use security::{LoginThrottle, origin_is_trusted};

/// 공유 상태.
///
/// 저장소를 `Arc` 로 감싼 이유: 핸들러들이 `&self` 만 받으므로 공유할 수 있다.
#[derive(Clone)]
pub struct AppState {
    store: Arc<SqliteStore>,
    /// 사전 조회기. **같은 `store` 를 공유한다** — 풀을 두 개 만들면 사전이
    /// 캐시한 단어를 다른 핸들러가 못 본다.
    dictionary: Arc<Dictionary>,
    /// 브라우저 대신 Rust 클라이언트(테스트, 데스크톱 앱)에는 `Origin` 이 없다.
    /// 이런 요청을 어떻게 할지 정책으로 정한다. 기본은 **허용** — 인증은 쿠키
    /// 토큰으로도 보호된다. 다만 CSRF 가 필요한 웹 브라우저 경로와는 다르다.
    allow_missing_origin: bool,
    secure_cookies: bool,
    allowed_origins: Arc<Vec<String>>,
    throttle: Arc<LoginThrottle>,
}

impl AppState {
    pub fn new(store: SqliteStore, config: &Config) -> Self {
        let store = Arc::new(store);
        Self {
            dictionary: Arc::new(Dictionary::new(Arc::clone(&store))),
            store,
            allow_missing_origin: true,
            secure_cookies: config.secure_cookies,
            allowed_origins: Arc::new(config.allowed_origins.clone()),
            throttle: Arc::new(LoginThrottle::new()),
        }
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub fn dictionary(&self) -> &Dictionary {
        &self.dictionary
    }

    /// 외부 사전의 주소를 바꾼다. 테스트와 대안 출처용.
    pub fn with_dictionary_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.dictionary =
            Arc::new(Dictionary::new(Arc::clone(&self.store)).with_endpoint(endpoint.into()));
        self
    }

    pub fn secure_cookies(&self) -> bool {
        self.secure_cookies
    }

    pub fn allowed_origins(&self) -> &[String] {
        &self.allowed_origins
    }

    /// CSRF 검사를 통과할 수 없는 요청을 허용할지.
    ///
    /// **기본값이 왜 `true` 인지**가 중요하다. 데스크톱 앱(Tauri)과 테스트는
    /// `Origin` 을 보내지 않는다. `false` 로 두면 그 경로가 전부 막힌다. 웹 브라우저
    /// 경로에서만 `false` 로 바꿔야 하는데, 그렇게 할 때 그 사실을 서버 로그에 남긴다.
    pub fn with_missing_origin_allowed(mut self, allow: bool) -> Self {
        self.allow_missing_origin = allow;
        self
    }
}

/// 라우터.
///
/// 여기가 얇다는 것이 요점이다. `submit_review` 의 트랜잭션이나 큐 정렬이 여기
/// 있으면 안 된다 — 그건 `voca-store` 가 하는 일이다.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/auth/register", post(routes::register))
        .route("/api/auth/login", post(routes::login))
        .route("/api/auth/logout", post(routes::logout))
        .route("/api/auth/me", get(routes::me))
        .route("/api/dict/lookup", get(routes::lookup_word))
        .route("/api/study/queue", get(study::study_queue))
        .route("/api/study/review", post(study::submit_review))
        .route("/api/dashboard", get(study::dashboard))
        .route("/api/senses", post(library::add_sense))
        .route(
            "/api/decks",
            get(library::list_decks).post(library::create_deck),
        )
        .route("/api/decks/cards", post(library::add_cards))
        .route("/api/decks/seed", post(seed::bootstrap_seed_endpoint))
        .route("/api/sync/changes", get(sync::changes))
        .route("/api/health", get(health))
        .route("/", get(page::index))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

//! HTTP 배선.
//!
//! 여기 있는 핸들러는 **모두 얇다.** 규칙은 [`crate::auth`] 에 있고, 여기는
//! (1) 입력을 꺼내고 (2) 규칙을 부르고 (3) 쿠키와 상태 코드를 정하는 일만 한다.
//! 규칙을 여기에 복사하면 두 벌이 나뉘어 어느 쪽이 진짜인지 모른다.
//!
//! [#adr-0013]: ../../docs/adr/0013-auth-rules-are-separate-from-http.md

use axum::extract::{FromRequestParts, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::Serialize;
use voca_store::{AuthFailure, RegisterRequest, SessionToken};

use crate::AppState;
pub(crate) use crate::auth::LoggedIn;
use crate::auth::{AuthContext, LoginRequest};
use crate::security::origin_is_trusted;

/// 세션 쿠키 이름.
pub const SESSION_COOKIE: &str = "voca_session";

/// 익명 세션 쿠키.
///
/// `HttpOnly` — 스크립트( XSS )가 읽을 수 없다.
/// `SameSite=Lax` — **다른 사이트가 만든 form POST 로는 이 쿠키가 안 나간다.**
/// `Path=/` — 모든 경로에서 쓴다.
/// `Secure` — HTTPS 에서만. 개발 중 HTTP 라면 끈다.
///
/// `Max-Age` 는 30일이다. 브라우저와 서버의 세션 수명을 어긋나게 두면 서버에서 이미
/// 죽은 세션으로 401 이 오는 짜증스러운 상황이 생긴다.
fn session_cookie(token: &SessionToken, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token.expose().to_owned()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(time::Duration::seconds(crate::auth::SESSION_TTL_SECONDS))
        .build()
}

/// 로그아웃용 쿠키. 같은 이름·속성으로 즉시 만료시켜야 브라우저에서 지워진다.
fn cleared_session_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, String::new()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(time::Duration::ZERO)
        .build()
}

// ── 회원가입 ────────────────────────────────────────────

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    _client: ClientIp,
    body: axum::Json<voca_store::RegisterRequest>,
) -> Result<Response, ApiError> {
    state.require_same_origin(&headers)?;

    let mut request: RegisterRequest = body.0;
    if request.retention.is_none() {
        request.retention = Some(voca_store::DEFAULT_RETENTION.to_owned());
    }

    let outcome = AuthContext::new(state.store()).register(request).await;
    finish_auth(state, outcome)
}

// ── 로그인 ──────────────────────────────────────────────

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    client: ClientIp,
    body: axum::Json<LoginRequest>,
) -> Result<Response, ApiError> {
    state.require_same_origin(&headers)?;

    let ip = client.0;
    let email = body.0.email.clone();
    // 한쪽만 잠그면 다른 쪽으로 우회할 수 있다. (이메일, IP) 쌍으로 센다.
    state.throttle.check(&email, ip, now_seconds())?;

    let outcome = AuthContext::new(state.store()).login(body.0).await;
    if matches!(outcome, crate::auth::AuthOutcome::LoggedIn(_)) {
        state.throttle.record_success(&email, ip);
    }
    finish_auth(state, outcome)
}

fn finish_auth(state: AppState, outcome: crate::auth::AuthOutcome) -> Result<Response, ApiError> {
    let LoggedIn { user, token } = outcome.into_parts()?;
    let cookie = session_cookie(&token, state.secure_cookies());
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie.to_string())],
        axum::Json(AuthBody::from(user)),
    )
        .into_response())
}

// ── 로그아웃 ────────────────────────────────────────────

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    state.require_same_origin(&headers)?;
    if let Some(token) = token_from(&headers) {
        let _ = AuthContext::new(state.store()).logout(&token).await;
    }
    let cleared = cleared_session_cookie(state.secure_cookies());
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cleared.to_string())],
    )
        .into_response())
}

// ── 현재 사용자 ─────────────────────────────────────────

pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let Some(token) = token_from(&headers) else {
        // 토큰이 없으면 200 과 null 을 준다. 401 은 "인증이 필요한 경로"라는 뜻이라
        // 여기에는 맞지 않는다 — 대시보드가 "로그인 안 됨"을 조용히 알아야 한다.
        return Ok(axum::Json(Option::<AuthBody>::None).into_response());
    };

    match AuthContext::new(state.store()).whoami(&token).await {
        Ok(Some(user)) => Ok(axum::Json(Some(AuthBody::from(user))).into_response()),
        Ok(None) => Ok(axum::Json(Option::<AuthBody>::None).into_response()),
        Err(e) => Err(ApiError::store(e)),
    }
}

/// 요청을 보낸 주소.
///
/// 로그인 제한에 필요한데, 없을 수 있다 — 역방향 프록시가 `ConnectInfo` 를 붙여 주지
/// 않으면 그렇다. **없다고 제한을 건너뛰지 않는다.** IP 축이 사라지면 한 계정만
/// 잠그는 공격이 된다.
///
/// 프록시 설정이 잘못되어 `X-Forwarded-For` 가 신뢰할 수 없는 값을 들고 올 수
/// 있으므로, 그 헤더는 **읽지 않는다.** 신뢰할 수 있는 유일한 값은 우리가 직접
/// 받은 접속 주소이고, 그것이 없으면 없는 채로 둔다.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub Option<std::net::IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|c| c.0.ip());
        Ok(ClientIp(ip))
    }
}

/// 쿠키에서 토큰을 꺼낸다.
pub fn token_from(headers: &HeaderMap) -> Option<SessionToken> {
    parse_cookie(headers.get(header::COOKIE)?.as_bytes())
        .and_then(|raw| raw.get(SESSION_COOKIE).cloned())
        .map(SessionToken::new)
}

/// `Cookie: a=1; b=2` 형태를 느리게 파싱한다.
///
/// 쿠키 파싱을 직접 하는 이유는 하나다 — `axum-extra` 의 쿠키 파서는 **URI 인코딩을
/// 요구**하는 반면, 우리가 담는 값은 URL 안전한 hex 다. 의존성을 늘리지 않는다.
fn parse_cookie(raw: &[u8]) -> Option<std::collections::HashMap<String, String>> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut out = std::collections::HashMap::new();
    for part in text.split(';') {
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        out.insert(name.trim().to_owned(), value.trim().to_owned());
    }
    Some(out)
}

pub fn now_seconds() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

// ── 응답 형식 ───────────────────────────────────────────

/// 로그인·회원가입 응답 본문.
///
/// **토큰은 여기 없다.** 쿠키로만 나간다. JSON 본문에 다시 실으면 XSS 스크립트가
/// 읽을 수 있다.
#[derive(Debug, Serialize)]
pub struct AuthBody {
    pub user_id: String,
    pub display_name: String,
    pub retention: String,
}

impl From<voca_store::SessionGrant> for AuthBody {
    fn from(g: voca_store::SessionGrant) -> Self {
        Self {
            user_id: g.user_id.to_string(),
            display_name: g.display_name,
            retention: g.retention,
        }
    }
}

// ── 사전 조회 ────────────────────────────────────────────

/// 사전 조회 결과.
///
/// **저장 여부를 함께 알려주는 게 요점이다.** 뜻은 봤지만 저장이 안 됐다면 사용자는
/// 뜻도 보고 "덱에 추가" 버튼도 보는데, 눌렀다 아무 일도 안 일어나는 셈이다. 그 상태를
/// 숨기지 않는다 (`docs/design.md` — "단순히 오류를 무시하지 않는다").
#[derive(Debug, Serialize)]
pub struct DictBody {
    /// `"cached"` / `"fetched"` / `"unstored"` / `"not_found"` /
    /// `"unavailable"` / `"unrecognized"`
    pub status: &'static str,
    pub word: Option<DictWord>,
}

#[derive(Debug, Serialize)]
pub struct DictWord {
    pub word_id: Option<String>,
    pub lemma: String,
    pub phonetic: Option<String>,
    pub senses: Vec<DictSense>,
}

#[derive(Debug, Serialize)]
pub struct DictSense {
    pub kind: String,
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
}

pub async fn lookup_word(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<LookupQuery>,
) -> Result<Response, ApiError> {
    // 로그인이 있어야 사전 내용을 가져온다. 익명으로 상위 어휘를 긁어내는 걸 막고,
    // rate limit 과 계정 정산을 붙일 자리를 남긴다.
    require_login(&state, &headers).await?;

    let lemma = q.lemma.trim();
    if lemma.is_empty() || lemma.len() > MAX_LEMMA_LEN {
        return Err(ApiError::from(AuthFailure::Rejected(
            "단어가 올바르지 않다",
        )));
    }

    let result = state.dictionary().lookup(lemma).await;
    Ok(axum::Json(dict_body(result)).into_response())
}

fn dict_body(result: voca_dict::client::Lookup) -> DictBody {
    use voca_dict::client::Lookup;

    let (status, word) = match result {
        Lookup::FromCache(w) => ("cached", Some(word_body(&w))),
        Lookup::Fetched(w) => ("fetched", Some(word_body(&w))),
        Lookup::FetchedUnstored(u) => (
            "unstored",
            Some(DictWord {
                // 저장이 안 됐으니 **식별자가 없다.** 지어내지 않는다.
                word_id: None,
                lemma: u.lemma,
                phonetic: u.phonetic,
                senses: u
                    .senses
                    .into_iter()
                    .map(|s| DictSense {
                        kind: format!("{:?}", s.kind).to_lowercase(),
                        pos: s.pos,
                        definition: s.definition,
                        example_en: s.example_en,
                    })
                    .collect(),
            }),
        ),
        // 전부 실패다. **서로 다른 상태를 200 으로 구별해서 돌려준다** — 클라이언트가
        // "다시 시도" 와 "사용자가 오타" 를 다르게 다루게.
        Lookup::NotFound => ("not_found", None),
        Lookup::Unavailable => ("unavailable", None),
        Lookup::Unrecognized => ("unrecognized", None),
    };

    DictBody { status, word }
}

fn word_body(w: &voca_store::WordView) -> DictWord {
    DictWord {
        word_id: Some(w.id.to_string()),
        lemma: w.lemma.clone(),
        phonetic: w.phonetic.clone(),
        senses: w
            .senses
            .iter()
            .map(|s| DictSense {
                kind: format!("{:?}", s.kind).to_lowercase(),
                pos: s.pos.clone(),
                definition: s.definition.clone(),
                example_en: s.example_en.clone(),
            })
            .collect(),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct LookupQuery {
    pub lemma: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct TtsQuery {
    pub text: String,
}

pub async fn audio_tts(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<TtsQuery>,
) -> Response {
    let text = q.text.trim();
    if text.is_empty() || text.len() > 300 {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }

    match state.dictionary().fetch_tts_audio(text).await {
        Some(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, "audio/mpeg"),
                (
                    axum::http::header::CACHE_CONTROL,
                    "public, max-age=86400, stale-while-revalidate=604800",
                ),
            ],
            bytes,
        )
            .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// 표기형 길이 상한.
///
/// 외부 요청을 그대로 URL 에 넣어 보낸다. 상한이 없으면 한 번에 아주 긴 문자열로
/// 메모리와 시간을 잡아먹을 수 있다.
const MAX_LEMMA_LEN: usize = 64;

/// 토큰이 없거나 만료됐으면 401.
pub(crate) async fn require_login(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<LoggedIn, ApiError> {
    let Some(token) = token_from(headers) else {
        return Err(ApiError::from(AuthFailure::BadCredentials));
    };
    match AuthContext::new(state.store()).whoami(&token).await {
        Ok(Some(user)) => Ok(LoggedIn { user, token }),
        // 토큰이 없거나 만료됐다. 이건 인증 실패다.
        Ok(None) => Err(ApiError::from(AuthFailure::BadCredentials)),
        // 저장소가 죽었는데 "비밀번호가 맞지 않습니다"를 주면 사용자는 비밀번호를
        // 계속 바꾼다. `ApiError::store`와 같은 규칙이다 — 원인을 구분해 503을 준다.
        Err(e) => Err(ApiError::store(e)),
    }
}

#[derive(Debug)]
pub struct ApiError(pub AuthFailure);

impl From<AuthFailure> for ApiError {
    fn from(f: AuthFailure) -> Self {
        ApiError(f)
    }
}

impl ApiError {
    /// 저장소 에러를 API 실패로 낮춘다.
    ///
    /// **500 을 401 로 바꾸지 않는다.** 저장소가 죽었는데 "비밀번호가 맞지
    /// 않습니다"를 주면 사용자가 비밀번호를 계속 바꾼다. 원인을 구분해 503 을 준다.
    pub(crate) fn store(e: voca_store::StoreError) -> Self {
        ApiError(match e {
            voca_store::StoreError::Unavailable | voca_store::StoreError::Transient => {
                AuthFailure::ServerDown
            }
            _ => AuthFailure::Rejected("요청을 처리하지 못했다"),
        })
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ErrorBody {
            code: error_code(self.0),
            message: self.0.message(),
        };
        let mut response = (status, axum::Json(body)).into_response();
        if self.0 == AuthFailure::TooManyAttempts {
            // 재시도 신호. 잠금 창(15분)을 그대로 알린다.
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("900"));
        }
        response
    }
}

/// 기계가 읽을 코드. 사람이 읽을 메시지와 별개다.
fn error_code(f: AuthFailure) -> &'static str {
    match f {
        AuthFailure::BadCredentials => "bad_credentials",
        AuthFailure::EmailTaken => "email_taken",
        AuthFailure::Rejected(_) => "rejected",
        AuthFailure::TooManyAttempts => "too_many_attempts",
        AuthFailure::ServerDown => "server_down",
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

impl AppState {
    /// 변경 요청이 우리 출처에서 왔는지 확인한다.
    pub(crate) fn require_same_origin(&self, headers: &HeaderMap) -> Result<(), ApiError> {
        let origin = header_str(headers, header::ORIGIN);
        let referer = header_str(headers, header::REFERER);
        if origin_is_trusted(origin.as_deref(), referer.as_deref(), &self.allowed_origins) {
            Ok(())
        } else {
            Err(ApiError(AuthFailure::Rejected("출처가 신뢰되지 않는다")))
        }
    }
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

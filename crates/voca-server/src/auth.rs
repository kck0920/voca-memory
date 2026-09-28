//! 인증 규칙.
//!
//! 여기는 **프레임워크를 모른다.** axum 핸들러도 Leptos 서버 함수도 아닌 순수 함수다.
//! HTTP 는 얇은 어댑터가 이 위에 얹힌다. 그래야 규칙을 웹 서버 없이 시험할 수 있다.
//!
//! 보안 규칙 두 가지가 이 모듈 전부를 관통한다:
//!
//! 1. **계정 존재 여부를 새지 않는다.** 미가입 이메일과 틀린 비밀번호는 같은 상태
//!    코드와 같은 메시지로 돌아간다. 시간으로도 구별되지 않게 한다 — 미가입이면
//!    해시 검증을 생략하면 안 되고, 시간 맞추기용 더미 검증을 돌린다.
//! 2. **토큰은 쿠키로만 나간다.** 응답 본문에 다시 실으면 XSS 스크립트에 노출된다.

use voca_store::{
    Accounts, AuthFailure, NewAccount, RegisterRequest, SessionGrant, SessionToken, Sessions,
    StoreError, StoreResult, UserId,
};

use crate::password;

/// 세션 유효 기간. 30일.
///
/// "기억하기"를 기본값으로 둔다 — 이 앱은 매일 쓰는 도구인데 매번 로그인시키면
/// 스트릭이 끊기는 원인이 된다. 위험은 `revoke_all_for` 로 낮춘다.
pub const SESSION_TTL_SECONDS: i64 = 30 * 86_400;

/// 로그인에 쓰이는 요청.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// 로그인에 성공했을 때의 결과.
///
/// `token` 은 **HTTP 쿠키로만** 전달해야 한다. JSON 응답에 실으면 XSS 가 스크립트로
/// 읽을 수 있다. `Debug` 는 토큰을 가린다(`SessionToken` 쪽에서 처리).
#[derive(Debug)]
pub struct LoggedIn {
    pub user: SessionGrant,
    pub token: SessionToken,
}

impl LoggedIn {
    /// 로그인·회원가입이 성공했을 때의 결과.
    pub fn user(&self) -> &SessionGrant {
        &self.user
    }
}

/// 인증 결과.
#[derive(Debug)]
pub enum AuthOutcome {
    LoggedIn(LoggedIn),
    Failed(AuthFailure),
}

impl AuthOutcome {
    pub fn into_parts(self) -> Result<LoggedIn, AuthFailure> {
        match self {
            AuthOutcome::LoggedIn(l) => Ok(l),
            AuthOutcome::Failed(f) => Err(f),
        }
    }
}

/// 인증 컨텍스트.
///
/// **제네릭인 이유:** `async fn` 이 들어간 트레잇은 `dyn` 호환이 되지 않는다.
/// `Box<dyn Future>` 를 넣는 `#[async_trait]` 는 매 호출마다 할당이 생기므로 쓰지
/// 않는다. 그래서 하나를 나열한다. `Store`·`Accounts`·`Sessions` 를 모두 만족하는
/// 타입이면 무엇이든 들어온다 — 어댑터가 갈라지면 세 개를 함께 갈라야 하는데,
/// 그게 어차피 같이 바뀌어야 하는 것들이기도 하다.
pub struct AuthContext<'a, T> {
    pub store: &'a T,
}

impl<'a, T> AuthContext<'a, T>
where
    T: Accounts + Sessions,
{
    pub fn new(store: &'a T) -> Self {
        Self { store }
    }
    /// 새 계정을 만들고 세션을 발급한다.
    pub async fn register(&self, request: RegisterRequest) -> AuthOutcome {
        if let Err(failure) = validate_registration(&request) {
            return AuthOutcome::Failed(failure);
        }

        // 해시는 저장 **전에** 만든다. 저장에 실패하면 만든 해시가 버려진다 —
        // 평문이 어디에도 남지 않는다.
        let password_hash = match password::hash(&request.password) {
            Ok(h) => h,
            Err(_) => {
                return AuthOutcome::Failed(AuthFailure::Rejected("비밀번호 길이가 맞지 않는다"));
            }
        };

        let input = NewAccount {
            id: UserId::from(uuid::Uuid::new_v4()),
            email: request.email,
            password_hash,
            display_name: request.display_name,
            timezone: request.timezone,
            retention: request.retention,
        };

        let user = match Accounts::create(self.store, input).await {
            Ok(u) => u,
            Err(StoreError::Integrity) => return AuthOutcome::Failed(AuthFailure::EmailTaken),
            Err(e) => return AuthOutcome::Failed(from_store(e)),
        };

        self.finish(user).await
    }

    /// 로그인한다.
    pub async fn login(&self, request: LoginRequest) -> AuthOutcome {
        // 후보가 지나치게 길면 argon2 에 넘기지 않는다. 계정 존재 여부를 먼저
        // 확인하면 시간 차로 그 사실이 새니까, 확인 없이 동일 시간만 쓴다.
        if request.password.len() > password::MAX_PASSWORD_BYTES {
            password::burn_time();
            return AuthOutcome::Failed(AuthFailure::BadCredentials);
        }

        let credential = match self.store.find_by_email(&request.email).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                // **미가입이면 실제로 하는 일이 없다.** 그런데 해시 검증을 생략하면
                // 응답 시간이 무참하게 짧아져 "이 이메일은 가입돼 있다"가 드러난다.
                // 그래서 같은 비용의 더미 검증을 돌린다.
                password::burn_time();
                return AuthOutcome::Failed(AuthFailure::BadCredentials);
            }
            Err(e) => return AuthOutcome::Failed(from_store(e)),
        };

        if !password::verify(&credential.password_hash, &request.password) {
            return AuthOutcome::Failed(AuthFailure::BadCredentials);
        }

        self.finish(credential.grant()).await
    }

    /// 로그아웃. 토큰이 이미 죽었어도 성공한다 — 재요청에 안전해야 한다.
    pub async fn logout(&self, token: &SessionToken) -> StoreResult<()> {
        self.store.revoke(token).await
    }

    /// 토큰으로 현재 사용자를 되살린다.
    pub async fn whoami(&self, token: &SessionToken) -> StoreResult<Option<SessionGrant>> {
        let Some(user) = self.store.resolve(token).await? else {
            return Ok(None);
        };
        // 계정이 지워졌다면 세션도 외래 키로 함께 지워졌어야 한다. 그래도 여기서
        // 확인한다 — 세션이 살아 있는데 계정이 없으면 잘못된 상태다.
        match self.store.find_by_id(user).await? {
            Some(c) => Ok(Some(c.grant())),
            None => Ok(None),
        }
    }

    /// "다른 기기에서 모두 로그아웃".
    pub async fn logout_everywhere(&self, user: UserId) -> StoreResult<()> {
        self.store.revoke_all_for(user).await
    }

    async fn finish(&self, user: SessionGrant) -> AuthOutcome {
        match Sessions::create(self.store, user.user_id, SESSION_TTL_SECONDS).await {
            Ok(token) => AuthOutcome::LoggedIn(LoggedIn { user, token }),
            Err(e) => AuthOutcome::Failed(from_store(e)),
        }
    }
}

/// 이메일이 형식에 맞는지 본다.
///
/// 완전한 RFC 5322 파서를 쓰지 않는다 — 규격대로 하면 개인 프로젝트에 과하고,
/// 이 앱에 들어오는 입력(사용자가 직접 친 것)에는 최소한으로 충분하다.
///
/// 거절해야 하는 것을 세 가지로 나눈다.
/// - `@` 가 정확히 하나가 아니다 (`"두개@골뱅이@com"`, `"이름@"`은 다른 이유다)
/// - **로컬 부분**이 비었거나, 점이 앞뒤를 두드리거나 연속한다
/// - **도메인**이 비었거나, 점이 없거나, 점이 앞뒤를 두드리거나 연속한다
///
/// 도메인만 보고 `.` 을 찾는 것으로는 `".시작점@example.com"` 이 통과해 버린다 —
/// 점이 있는 쪽은 로컬 부분이다.
fn is_email_shaped(raw: &str) -> bool {
    let email = raw.trim();
    if email.is_empty() || email.chars().any(char::is_whitespace) {
        return false;
    }

    let mut parts = email.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty() || domain.is_empty() {
        return false;
    }

    let dots_are_sane =
        |part: &str| !part.contains("..") && !part.starts_with('.') && !part.ends_with('.');
    if !dots_are_sane(local) {
        return false;
    }

    // 도메인에는 점이 있어야 하고, TLD 가 비면 안 된다.
    match domain.rfind('.') {
        Some(dot) => dot > 0 && dot + 1 < domain.len() && dots_are_sane(domain),
        None => false,
    }
}

fn validate_registration(request: &RegisterRequest) -> Result<(), AuthFailure> {
    let email = request.email.trim();
    if !is_email_shaped(email) {
        return Err(AuthFailure::Rejected("이메일 형식이 아니다"));
    }
    if request.display_name.trim().is_empty() {
        return Err(AuthFailure::Rejected("이름이 비었다"));
    }
    if request.timezone.trim().is_empty() {
        return Err(AuthFailure::Rejected("시간대가 비었다"));
    }
    Ok(())
}

/// 저장소 에러를 인증 실패로 낮춘다.
///
/// **서버가 죽은 것과 입력이 틀린 것을 같은 응답으로 돌려주면 안 된다.** 앞자는
/// 재시도하면 되고 뒤자는 재시도해 소용없다. `ServerDown`(503) 으로 올려 질식을
/// 만든다.
fn from_store(e: StoreError) -> AuthFailure {
    match e {
        StoreError::Unavailable | StoreError::Transient => AuthFailure::ServerDown,
        _ => AuthFailure::Rejected("요청을 처리하지 못했다"),
    }
}

#[cfg(test)]
mod tests {
    use super::is_email_shaped;

    #[test]
    fn ordinary_addresses_pass() {
        for e in [
            "a@b.kr",
            "someone@example.com",
            "first.last@sub.example.co.kr",
            "  someone@example.com  ",
            "user+tag@example.com",
            "user_name@example.com",
            "숫자123@example.com",
            // 앞뒤 공백은 **정리한다** — 거절이 아니다. 붙여넣기가 흔하다.
            "someone@example.com ",
        ] {
            assert!(is_email_shaped(e), "거절했다: {e:?}");
        }
    }

    #[test]
    fn obviously_broken_addresses_are_refused() {
        for e in [
            "",
            "   ",
            // 공백이 **중간에** 있으면 거절한다. 앞뒤 공백은 위에서 정리한다.
            "공백 있음@b.kr",
            "이름@",
            "@예.kr",
            "@",
            "plainaddress",
            "두개@골뱅이@com",
            "끝에점@.",
            ".시작점@example.com",
            "연속점..@example.com",
            "도메인..연속@example.com",
        ] {
            assert!(!is_email_shaped(e), "통과했다: {e:?}");
        }
    }
}

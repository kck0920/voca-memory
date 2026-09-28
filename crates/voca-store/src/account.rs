use serde::{Deserialize, Serialize};

use crate::StoreResult;
use crate::session::SessionGrant;

/// 계정. `Store` 와 분리된 interface 다.
///
/// `Store` 는 학습 자료를 다룬다. 이것은 신원을 다룬다. **바뀌는 이유가 다르고
/// 바뀌는 시점도 다르다** — 스케줄링 규칙을 손볼 때 계정 테이블을 열 필요는 없고,
/// 그 반대도 마찬가지다. 그래서 같은 어댑터 안에서도 별도 interface 로 둔다.
#[allow(async_fn_in_trait)]
pub trait Accounts: Send + Sync {
    /// 새 계정을 만든다. 이메일이 이미 있으면 `Integrity` 다.
    ///
    /// `password_hash` 는 이미 해시된 값이어야 한다. **평문을 받으면 안 된다** —
    /// 해시 파라미터를 바꾸려면 계정 전부를 다시 해시해야 하는데, 그게 가능하려면
    /// 평문이 어딘가에 있어야 한다.
    async fn create(&self, input: NewAccount) -> StoreResult<SessionGrant>;

    /// 이메일로 계정을 찾는다. 비밀번호 검증은 하지 않는다 — 해시 알고리즘을
    /// 모르는 서버가 그럴 수 있어야 하기 때문이다.
    async fn find_by_email(&self, email: &str) -> StoreResult<Option<Credential>>;

    /// 식별자로 계정을 찾는다. 세션이 살아 있을 때 현재 사용자를 되살릴 때 쓴다.
    async fn find_by_id(&self, id: crate::UserId) -> StoreResult<Option<Credential>>;

    /// 계정 수. 관리용. 일반 요청 경로에서는 쓰지 않는다.
    async fn count(&self) -> StoreResult<u64>;
}

/// 새로 만들 계정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub id: crate::UserId,
    pub email: String,
    /// `password-hash` 의 PHC 문자열. 평문이 아니다.
    pub password_hash: String,
    pub display_name: String,
    /// IANA 시간대. **표시 전용.**
    pub timezone: String,
    /// `diligent | balanced | frugal`. `None` 이면 서버 기본값.
    pub retention: Option<String>,
}

/// `retention` 이 없을 때 쓰는 기본값.
///
/// 서버와 저장소가 같은 값을 써야 계정을 만든 쪽과 읽는 쪽이 어긋나지 않는다.
pub const DEFAULT_RETENTION: &str = "balanced";

/// 로그인 검증에 필요한 최소한.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub user_id: crate::UserId,
    /// PHC 문자열.
    pub password_hash: String,
    pub display_name: String,
    pub retention: String,
    pub created_at: i64,
}

impl Credential {
    /// 세션 발급에 필요한 것을 뺀다.
    ///
    /// 비밀번호 해시는 여기서 떨어져 나간다. 이 구조체를 로그에 남기거나 직렬화할
    /// 위험이 사라진다.
    pub fn grant(&self) -> SessionGrant {
        SessionGrant {
            user_id: self.user_id,
            display_name: self.display_name.clone(),
            retention: self.retention.clone(),
        }
    }
}

/// 로그인 실패.
///
/// **존재하지 않는 이메일과 틀린 비밀번호를 구분하지 않는다.** 구분해서 돌려주면
/// 공격자가 "이 이메일은 가입돼 있다"를 알 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthFailure {
    /// 이메일이 없거나 비밀번호가 틀렸다. 둘을 구분하지 않는다.
    BadCredentials,
    /// 이메일이 이미 쓰이고 있다.
    EmailTaken,
    /// 입력이 형식을 어겼다 (이메일 형식, 비밀번호 길이 등).
    Rejected(&'static str),
    /// 시도가 너무 많다. 잠시 뒤 다시 시도해야 한다.
    ///
    /// **계정 정보를 새지 않는다.** 잠긴 대상이 특정 이메일이면 "이 이메일은
    /// 가입돼 있다"가 된다. 메시지도 `BadCredentials` 와 같게 두고 상태 코드만
    /// 429 로 올린다.
    TooManyAttempts,
    /// 저장소가 응답하지 않는다.
    ///
    /// **재시도할 수 있는 실패다.** 비밀번호 오류와 구분해야 한다 — 사용자가
    /// 제대로 입력했는데 서버가 죽었는데 "비밀번호가 틀렸습니다"를 받으면 아무것도
    /// 할 수 없다.
    ServerDown,
}

impl AuthFailure {
    /// HTTP 상태 코드. `Rejected` 도 400 으로 내려간다 — 422 는 서버가 형식을
    /// 이해하지 못한 거라 클라이언트 버그를 뜻하게 되기 때문이다.
    pub fn status(&self) -> u16 {
        match self {
            AuthFailure::BadCredentials => 401,
            AuthFailure::EmailTaken => 409,
            AuthFailure::Rejected(_) => 400,
            AuthFailure::TooManyAttempts => 429,
            AuthFailure::ServerDown => 503,
        }
    }

    /// 사용자에게 보여줄 문구.
    ///
    /// 비밀번호 오류와 미가입을 같은 문구로 돌려주는 것이 규칙이다. 서버 로그에는
    /// 어느 쪽인지 남기지만 응답에는 새지 않는다.
    pub fn message(&self) -> &'static str {
        match self {
            AuthFailure::BadCredentials => "이메일 또는 비밀번호가 맞지 않습니다",
            AuthFailure::EmailTaken => "이미 사용 중인 이메일입니다",
            AuthFailure::Rejected(_) => "입력 형식이 맞지 않습니다",
            AuthFailure::TooManyAttempts => "이메일 또는 비밀번호가 맞지 않습니다",
            AuthFailure::ServerDown => "지금은 접속할 수 없습니다. 잠시 후 다시 시도해 주세요",
        }
    }

    /// 재시도하면 성공할 수 있는 실패인가.
    pub fn is_retryable(&self) -> bool {
        matches!(self, AuthFailure::ServerDown | AuthFailure::TooManyAttempts)
    }
}

/// 로그인·회원가입에 통과한 사용자.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authenticated {
    pub user: SessionGrant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_can_hand_out_a_grant_without_its_hash() {
        let c = Credential {
            user_id: crate::UserId::from([1u8; 16]),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$abcdef$hash".into(),
            display_name: "테스터".into(),
            retention: "balanced".into(),
            created_at: 0,
        };
        let grant = c.grant();
        assert_eq!(grant.display_name, "테스터");
        // grant 에 해시 필드가 아예 없다. 실수로 새는 경로가 존재하지 않는다.
        assert!(!format!("{grant:?}").contains("argon2"));
    }

    #[test]
    fn bad_credentials_and_a_missing_account_look_identical() {
        // 401 이고 메시지가 같아야 한다. 다르면 "이 이메일은 가입돼 있다"를 알린다.
        assert_eq!(AuthFailure::BadCredentials.status(), 401);
        assert_eq!(
            AuthFailure::BadCredentials.message(),
            "이메일 또는 비밀번호가 맞지 않습니다"
        );
    }

    #[test]
    fn rejection_is_a_client_error() {
        assert_eq!(AuthFailure::Rejected("비밀번호가 너무 짧다").status(), 400);
    }

    #[test]
    fn a_taken_email_is_a_conflict() {
        assert_eq!(AuthFailure::EmailTaken.status(), 409);
    }

    #[test]
    fn a_throttled_attempt_looks_like_a_bad_password_but_reports_429() {
        // 메시지는 같게 두고 상태 코드만 429 로 올린다 — 어느 이메일인지 새지 않으면서
        // 재시도하라는 신호는 준다.
        let f = AuthFailure::TooManyAttempts;
        assert_eq!(f.status(), 429);
        assert_eq!(f.message(), AuthFailure::BadCredentials.message());
        assert!(f.is_retryable());
    }

    #[test]
    fn a_dead_store_is_a_service_unavailable_not_a_bad_password() {
        // 사용자가 제대로 입력했는데 "비밀번호가 틀렸습니다"를 받으면 아무것도 할 수 없다.
        let f = AuthFailure::ServerDown;
        assert_eq!(f.status(), 503);
        assert!(f.is_retryable());
        assert!(!AuthFailure::BadCredentials.is_retryable());
        assert!(!AuthFailure::EmailTaken.is_retryable());
    }
}

use serde::{Deserialize, Serialize};

use crate::{StoreResult, UserId};

/// 로그인 세션의 토큰.
///
/// 브라우저(쿠키)에는 **평문 토큰**이 있고, 저장소에는 그 해시만 있다. 그래서 이
/// 타입은 `Debug` 로 찍으면 안 되고 `Display` 도 없다 — 실수로 로그에 새면 끝이다.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// 새로 발급한 토큰을 감싼다. 호출자는 **무작위 생성한 값**만 넣어야 한다.
    pub fn new(raw: String) -> Self {
        Self(raw)
    }

    /// 평문 토큰. 쿠키에 넣을 때만 쓴다.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 값 대신 존재 여부만 찍는다. 로그에 토큰이 새면 안 된다.
        f.write_str("SessionToken(<redacted>)")
    }
}

/// 세션 하나가 요구하는 것.
///
/// `Store` 에 붙이지 않고 **별도의 좁은 interface** 로 둔다. 세션은 도메인 자료가
/// 아니라 인증 인프라이고, `Store` 는 도메인 연산 단위다 (docs/adr/0012).
/// sqlite 접근을 `voca-server` 가 직접 하는 것도 싫다 — 그러면 seam 을 뚫는다.
#[allow(async_fn_in_trait)]
pub trait Sessions: Send + Sync {
    /// 새 세션을 만든다. 반환된 토큰은 **한 번만** 그 사용자에게 전달한다 —
    /// 해시만 저장되므로 다시 조회할 수 없다.
    async fn create(&self, user: UserId, ttl_seconds: i64) -> StoreResult<SessionToken>;

    /// 토큰으로 사용자를 찾는다. 만료됐으면 `None`.
    ///
    /// 성공하면 `seen_at` 을 갱신한다 — 이것이 구현의 책임이다.
    async fn resolve(&self, token: &SessionToken) -> StoreResult<Option<UserId>>;

    /// 로그아웃. 토큰이 이미 없어도 성공한다 — 재요청에 idempotent해야 한다.
    async fn revoke(&self, token: &SessionToken) -> StoreResult<()>;

    /// 특정 사용자의 세션을 전부 끊는다. "다른 기기에서 모두 로그아웃"에 쓴다.
    async fn revoke_all_for(&self, user: UserId) -> StoreResult<()>;

    /// 만료된 세션을 지운다. 예약 작업에서 주기적으로 부른다.
    async fn purge_expired(&self) -> StoreResult<u64>;
}

/// 로그인에 성공한 사용자에게 돌려주는 것.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGrant {
    pub user_id: UserId,
    pub display_name: String,
    pub retention: String,
}

/// 회원가입 요청.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    /// IANA 시간대. **표시 전용** — Streak 판정에는 쓰지 않는다.
    pub timezone: String,
    /// 목표 기억 유지율 프리셋. 없으면 `balanced`.
    pub retention: Option<String>,
}

/// 로그인 요청.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// 로그인·회원가입이 성공했을 때 돌려주는 것.
///
/// 토큰은 쿠키로만 나간다. 응답 본문에 다시 실으면 XSS 로 스크립트에 노출된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthResult {
    pub user: SessionGrant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_never_prints_itself() {
        let token = SessionToken::new("super-secret-value".into());
        let shown = format!("{token:?}");
        assert!(
            !shown.contains("super-secret-value"),
            "토큰이 {:?} 에 새었다",
            shown
        );
        assert!(shown.contains("redacted"));
    }

    #[test]
    fn the_plain_token_is_only_reachable_on_purpose() {
        let token = SessionToken::new("abc".into());
        assert_eq!(token.expose(), "abc");
    }
}

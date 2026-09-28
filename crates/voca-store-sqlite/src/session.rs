use sha2::{Digest, Sha256};
use voca_store::{SessionToken, Sessions, StoreError, StoreResult, UserId};

use crate::SqliteStore;

/// 세션 토큰의 바이트 수.
///
/// 256비트다. 무작위 예측 가능성은 무시할 만하지만, 짧게 잡는 것이 습관이라
/// 명시적으로 박아 둔다.
const TOKEN_BYTES: usize = 32;

impl SqliteStore {
    /// 세션용 난수 바이트를 뽑는다.
    ///
    /// `getrandom` 을 직접 쓴다. `rand` 를 끌어오면 의존성이 늘고, 이 한 가지 용도
    /// 에는 과하다.
    fn random_bytes() -> StoreResult<[u8; TOKEN_BYTES]> {
        let mut out = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut out).map_err(|_| StoreError::Unavailable)?;
        Ok(out)
    }
}

/// 토큰을 저장용 해시로 바꾼다.
///
/// **해시 조회는 DB 유출자를 무력화하지 않는다** — 토큰을 해시한다는 사실 자체가
/// 공개되면 오프라인으로 후보를 짤 수 있기 때문이다. 그럼에도 해시하는 이유는
/// "DB 만 새면 세션이 전부 살아 있다"는 단순한 경로 하나를 없애는 것이다.
fn hash(token: &SessionToken) -> [u8; 32] {
    let digest = Sha256::digest(token.expose().as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

impl Sessions for SqliteStore {
    async fn create(&self, user: UserId, ttl_seconds: i64) -> StoreResult<SessionToken> {
        let raw = Self::random_bytes()?;
        let token = SessionToken::new(hex(&raw));
        let now = crate::now_unix_seconds();
        // 만료 시각이 이미 지난 ttl 이면 그 세션은 곧 죽는다. 방어적으로 60초를
        // 최소값으로 둔다 — 0 을 넣으면 "지금 만든 유효 세션"이 될 수도 있다.
        let ttl = ttl_seconds.max(60);

        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, seen_at, expires_at)
             VALUES (?1, ?2, ?3, ?3, ?3 + ?4)",
        )
        .bind(hash(&token).as_slice())
        .bind(user.to_string())
        .bind(now)
        .bind(ttl)
        .execute(self.pool())
        .await
        .map_err(crate::classify)?;

        Ok(token)
    }

    async fn resolve(&self, token: &SessionToken) -> StoreResult<Option<UserId>> {
        let now = crate::now_unix_seconds();

        // 갱신과 조회를 한 번에 한다. 두 번 왕복하면 그 사이에 다른 요청이 토큰을
        // 지울 수 있다.
        let user: Option<String> = sqlx::query_scalar(
            "UPDATE sessions SET seen_at = ?2
             WHERE token_hash = ?1 AND expires_at > ?2
             RETURNING user_id",
        )
        .bind(hash(token).as_slice())
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(crate::classify)?;

        let Some(raw) = user else {
            return Ok(None);
        };
        Ok(Some(
            voca_domain::Id::parse(&raw).map_err(|_| StoreError::Integrity)?,
        ))
    }

    async fn revoke(&self, token: &SessionToken) -> StoreResult<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?1")
            .bind(hash(token).as_slice())
            .execute(self.pool())
            .await
            .map_err(crate::classify)?;
        Ok(())
    }

    async fn revoke_all_for(&self, user: UserId) -> StoreResult<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
            .bind(user.to_string())
            .execute(self.pool())
            .await
            .map_err(crate::classify)?;
        Ok(())
    }

    async fn purge_expired(&self) -> StoreResult<u64> {
        let n = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?1")
            .bind(crate::now_unix_seconds())
            .execute(self.pool())
            .await
            .map_err(crate::classify)?
            .rows_affected();
        Ok(n as u64)
    }
}

fn hex(bytes: &[u8; TOKEN_BYTES]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(TOKEN_BYTES * 2);
    for b in bytes {
        // `write!` 는 String 에 절대 실패하지 않는다.
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_deterministic() {
        let t = SessionToken::new("abc".into());
        assert_eq!(hash(&t), hash(&t));
    }

    #[test]
    fn different_tokens_hash_differently() {
        assert_ne!(
            hash(&SessionToken::new("a".into())),
            hash(&SessionToken::new("b".into()))
        );
    }

    #[test]
    fn hex_is_lowercase_and_fixed_width() {
        let s = hex(&[
            0x0a, 0xff, 0x10, 0x00, 0xAB, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
            0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
            0xab, 0xcd, 0xef, 0x01,
        ]);
        assert_eq!(s.len(), 64);
        assert_eq!(&s[..4], "0aff");
    }

    #[test]
    fn a_token_is_256_bits_of_randomness() {
        // 토큰이 짧으면 무작위 검색으로 세션 탈취가 가능하다. 길이를 고정한다.
        let bytes = SqliteStore::random_bytes().unwrap();
        assert_eq!(hex(&bytes).len(), TOKEN_BYTES * 2);
    }

    #[test]
    fn two_tokens_differ() {
        // 난수원이 실제로 난수를 주는지 확인한다. `getrandom` 이 조용히 같은 값을
        // 돌려주는 사고를 잡는다.
        let a = hex(&SqliteStore::random_bytes().unwrap());
        let b = hex(&SqliteStore::random_bytes().unwrap());
        assert_ne!(a, b);
    }
}

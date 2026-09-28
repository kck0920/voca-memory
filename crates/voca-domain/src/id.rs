use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 도메인 엔티티의 식별자.
///
/// 이 타입은 파싱과 직렬화만 담당한다. **ID를 생성하지 않는다.**
/// 발급은 저장소 경계의 책임이다 — 도메인이 난수를 요구하게 되면 Wasm 빌드
/// 제약([`docs/adr/0003`](../../docs/adr/0003-voca-domain-is-wasm-safe-pure-rust.md))을
/// 다시 검토해야 할 이유가 생긴다.
///
/// `fsrs` 크레이트가 `rand`를 통해 `getrandom`을 끌어온다는 사실이 이 제약의 전부다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(Uuid);

impl Id {
    /// UUID 문자열을 파싱한다. `Uuid::parse_str`와 동일하다.
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(Self)
    }
}

impl From<Uuid> for Id {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<[u8; 16]> for Id {
    /// 이미 만들어진 16바이트에서 `Id`를 만든다. 테스트 픽스처와
    /// 바이트 단위 복제에 쓴다 — 버전을 난수로 뽑지는 않는다.
    fn from(value: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(value))
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Uuid {
        Uuid::parse_str("0195f0a0-0000-7000-8000-000000000001").unwrap()
    }

    #[test]
    fn round_trips_through_string() {
        let id = Id::from(sample());
        assert_eq!(id.to_string(), "0195f0a0-0000-7000-8000-000000000001");
        assert_eq!(Id::parse(&id.to_string()), Ok(id));
    }

    #[test]
    fn serializes_transparently() {
        let id = Id::from(sample());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"0195f0a0-0000-7000-8000-000000000001\"");
        assert_eq!(serde_json::from_str::<Id>(&json).unwrap(), id);
    }

    #[test]
    fn rejects_malformed_strings() {
        assert!(Id::parse("not-a-uuid").is_err());
    }
}

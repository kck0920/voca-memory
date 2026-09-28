use serde::{Deserialize, Serialize};

/// 동기화 대상 행이 변경될 때마다 증가하는 단조 카운터.
///
/// [`docs/adr/0005`](../../docs/adr/0005-revision-column-now-sync-later.md)에서 도입했다.
/// 이 타입은 `voca-domain`이 아니라 `voca-store`에 있다 — 도메인은 동기화 메커니즘을
/// 모른다. 다만 [`CONTEXT.md`](../../CONTEXT.md)에는 도메인 용어로 정의되어 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Revision = Revision(0);

    /// 아직 갱신된 적 없는 행의 초기값.
    pub const fn initial() -> Self {
        Revision(0)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// 다음 revision. 증가는 여기 한 군데서만 일어난다.
    pub const fn next(self) -> Self {
        Revision(self.0 + 1)
    }
}

impl From<u64> for Revision {
    fn from(value: u64) -> Self {
        Revision(value)
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// 행과 그 행의 동기화 메타데이터를 함께 담는다.
///
/// 도메인 타입을 동기화 구현으로 오염시키지 않으면서, **어떤 조회도 revision을
/// 챙기게** 만드는 장치다. 구조화 destructuring 없이 필드를 빼내려면 모듈러
/// 연산자가 필요하므로, `rev`만 빠뜨리는 실수는 컴파일 에러가 된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versioned<T> {
    pub item: T,
    pub revision: Revision,
}

impl<T> Versioned<T> {
    pub fn new(item: T, revision: Revision) -> Self {
        Self { item, revision }
    }

    /// 필드를 꺼내면서 revision을 버린다. 동기화 없이 값만 필요할 때.
    pub fn into_item(self) -> T {
        self.item
    }

    pub fn as_item(&self) -> &T {
        &self.item
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Versioned<U> {
        Versioned {
            item: f(self.item),
            revision: self.revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_increases_monotonically() {
        let mut r = Revision::initial();
        let mut previous = r;
        for _ in 0..1000 {
            r = r.next();
            assert!(r > previous);
            previous = r;
        }
        assert_eq!(r.as_u64(), 1000);
        assert_eq!(r.to_string(), "r1000");
    }

    #[test]
    fn versioned_keeps_the_revision_through_a_map() {
        let v = Versioned::new("abandon", Revision::from(7));
        let mapped = v.map(|s| s.len());
        assert_eq!(mapped.item, 7);
        assert_eq!(mapped.revision, Revision::from(7));
        assert_eq!(mapped.into_item(), 7);
    }

    #[test]
    fn initial_is_zero() {
        assert_eq!(Revision::INITIAL.as_u64(), 0);
    }
}

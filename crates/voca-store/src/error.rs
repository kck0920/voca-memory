use std::fmt;

/// 저장소가 실패할 때 이유.
///
/// 호출부가 **다시 시도해도 되는지**를 알 수 있어야 하므로, 분류를 작게 유지한다.
/// 무엇이 잘못됐는지 세부적으로 알려주는 에러는 구현이 로그로 남긴다 — 호출자는
/// 그걸로 무엇을 할 수 없다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// 요청한 대상이 없다. 정합성 위반이지 일시적 실패가 아니다.
    NotFound,
    /// 입력이 도메인 규칙을 어긴다. 빈 definition 같은 것.
    Invalid(&'static str),
    /// 참조 무결성이 깨졌다. foreign key가 안 맞는다.
    Integrity,
    /// 낙관적 동시성 충돌이 재시도 한도를 넘었다.
    ///
    /// 실제로는 일어나기 어렵다 — 같은 Card를 수십 회 동시 복습해야 한다.
    /// 일어나면 그대로 재전송해 사용자가 다시 눌러야 한다.
    ConflictExhausted,
    /// 저장소가 일시적으로 실패했다. 재시도 가능.
    Transient,
    /// 저장소가 다 죽었다. 재시도 불가.
    Unavailable,
}

impl StoreError {
    /// 재시도해도 되는가.
    pub fn is_retryable(&self) -> bool {
        matches!(self, StoreError::Transient)
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => f.write_str("대상을 찾을 수 없다"),
            StoreError::Invalid(what) => write!(f, "입력이 규칙을 어긴다: {what}"),
            StoreError::Integrity => f.write_str("참조 무결성이 깨졌다"),
            StoreError::ConflictExhausted => f.write_str("동시 갱신 충돌이 한도를 넘었다"),
            StoreError::Transient => f.write_str("일시적 실패"),
            StoreError::Unavailable => f.write_str("저장소를 쓸 수 없다"),
        }
    }
}

impl std::error::Error for StoreError {}

pub type StoreResult<T> = Result<T, StoreError>;

/// 도메인 검증이 저장소 경계에서 실패했을 때 만든다.
pub(crate) fn invalid(what: &'static str) -> StoreError {
    StoreError::Invalid(what)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_is_retryable() {
        assert!(StoreError::Transient.is_retryable());
        for error in [
            StoreError::NotFound,
            StoreError::Invalid("definition이 비었다"),
            StoreError::Integrity,
            StoreError::ConflictExhausted,
            StoreError::Unavailable,
        ] {
            assert!(
                !error.is_retryable(),
                "{error:?}를 재시도 가능으로 분류하면 안 된다"
            );
        }
    }

    #[test]
    fn messages_carry_the_rule_that_was_broken() {
        let e = invalid("definition이 비었다");
        assert!(e.to_string().contains("definition"));
    }
}

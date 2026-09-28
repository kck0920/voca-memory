use voca_domain::SenseKind;

use crate::StoreResult;

/// 뜻 검색 조건.
///
/// 빈 쿼리는 금지한다. `senses`는 커지는 쪽이니까 전체를 스트리밍으로 빼내면 되지만,
/// 그건 브라우저에서 화면에 필요한 양보다 훨씬 많다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SenseQuery {
    /// 표기형 또는 뜻 본문에 대한 부분 일치. 대소문자 무시.
    pub text: Option<String>,
    pub kinds: Vec<SenseKind>,
    /// `Some(true)`면 사용자 것만, `Some(false)`면 사전 것만, `None`이면 전부.
    pub source: Option<bool>,
    /// 특정 Word 안에서만 찾는다.
    pub word: Option<crate::WordId>,
    pub limit: u32,
    pub offset: u32,
}

pub const DEFAULT_LIMIT: u32 = 20;
pub const MAX_LIMIT: u32 = 200;

impl SenseQuery {
    /// 아무 조건 없는 전수 조회를 막는다. 검색 UI는 항상 빈 문자열로 시작하므로
    /// 그 상태로 저장소를 때리지 않게 빈 텍스트는 "조건 없음"이 아니라 오류로 본다.
    pub fn text(term: impl Into<String>) -> Self {
        Self {
            text: Some(term.into()),
            limit: DEFAULT_LIMIT,
            ..Self::default()
        }
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit.min(MAX_LIMIT);
        self
    }

    pub fn in_word(mut self, word: crate::WordId) -> Self {
        self.word = Some(word);
        self
    }

    pub fn only_user(mut self) -> Self {
        self.source = Some(true);
        self
    }

    pub fn only_dictionary(mut self) -> Self {
        self.source = Some(false);
        self
    }

    /// 조건이 하나도 없는지. 전체 스캔을 뜻한다.
    pub fn is_unfiltered(&self) -> bool {
        self.text.as_deref().unwrap_or("").trim().is_empty()
            && self.kinds.is_empty()
            && self.source.is_none()
            && self.word.is_none()
    }

    pub fn validate(&self) -> StoreResult<()> {
        if self.is_unfiltered() {
            return Err(crate::StoreError::Invalid(
                "검색 조건이 없다. 전체 스캔은 허용하지 않는다",
            ));
        }
        Ok(())
    }
}

/// 한 페이지의 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// 조건에 맞는 총 개수. `items.len()`과 다르다 — 다음 페이지가 있는지는 이것으로 안다.
    pub total: u32,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, total: u32) -> Self {
        Self { items, total }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 다음 페이지가 있는가.
    pub fn has_more(&self) -> bool {
        (self.items.len() as u32) < self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_text_query_is_valid() {
        assert!(SenseQuery::text("abandon").validate().is_ok());
    }

    #[test]
    fn an_empty_query_is_rejected() {
        assert!(SenseQuery::default().is_unfiltered());
        assert!(SenseQuery::default().validate().is_err());
    }

    #[test]
    fn whitespace_only_text_counts_as_unfiltered() {
        let q = SenseQuery::text("   ");
        assert!(
            q.is_unfiltered(),
            "공백만 입력하면 스캔이 되므로 막아야 한다"
        );
        assert!(q.validate().is_err());
    }

    #[test]
    fn any_narrowing_condition_makes_it_filtered() {
        let base = SenseQuery::text("   ");
        assert!(!base.clone().only_user().is_unfiltered());
        assert!(!SenseQuery::default().only_dictionary().is_unfiltered());
    }

    #[test]
    fn limit_is_capped_so_the_ui_cannot_ask_for_everything() {
        let q = SenseQuery::text("a").with_limit(10_000);
        assert_eq!(q.limit, MAX_LIMIT);
    }

    #[test]
    fn page_reports_whether_more_is_waiting() {
        let page = Page::new(vec![1, 2, 3], 3);
        assert!(!page.has_more());
        assert!(!page.is_empty());

        let page = Page::new(vec![1, 2, 3], 10);
        assert!(page.has_more());

        assert!(Page::<u8>::new(Vec::new(), 0).is_empty());
    }
}

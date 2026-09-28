use serde::Serialize;
use voca_domain::Id;

// 도메인 용어를 여기서 다시 정의하지 않는다. 아래 세 열거형은 `voca_domain` 의 것이
// 유일한 정본이다 — 같은 개념이 두 곳에 있으면 어느 쪽이 진짜인지 모른다. 두 정의가
// 대소문자 표기(`snake_case`) 와 serde 형식까지 같아야 하므로, 재선언은 반드시
// 조용히 어긋난다.
pub use voca_domain::{SenseKind, SenseSource, WordSource};

use crate::{SenseId, WordId};

/// 어 항목과 그 아래의 Sense들을 한 번에 읽은 것.
///
/// "단어를 추가" 화면은 Word 한 개와 그 뜻 목록을 함께 필요로 한다. 이 두 조회를
/// 합쳐 한 번의 왕복으로 끝낸다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WordView {
    pub id: WordId,
    pub lemma: String,
    pub source: WordSource,
    pub phonetic: Option<String>,
    pub audio_url: Option<String>,
    /// 이 Word에 붙은 Sense. 빈 배열은 없다 — Word는 Sense를 하나 이상 가진다.
    pub senses: Vec<SenseView>,
}

impl WordView {
    /// 조회 결과를 이불규칙 성질에 맞게 지킨다.
    pub fn new(
        id: WordId,
        lemma: String,
        source: WordSource,
        phonetic: Option<String>,
        audio_url: Option<String>,
        mut senses: Vec<SenseView>,
    ) -> Self {
        senses.sort_by_key(|s| s.id);
        Self {
            id,
            lemma,
            source,
            phonetic,
            audio_url,
            senses,
        }
    }

    /// 사용자가 직접 지은 항목인지.
    pub fn is_user_authored(&self) -> bool {
        self.source == WordSource::User
    }
}

/// 뜻 하나를 화면에 넘길 때 필요한 전부.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SenseView {
    pub id: SenseId,
    pub word_id: WordId,
    pub kind: SenseKind,
    pub source: SenseSource,
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
    /// 이 Sense의 Card가 이미 만들어진 덱들. "수능 단어"에 이미 넣었다면
    /// 중복 추가를 막는 데 쓴다.
    pub in_decks: Vec<Id>,
}

impl SenseView {
    /// 이 Sense가 어느 면을 Card의 앞면으로 보여주는가.
    ///
    /// `example`은 예문을, 나머지는 그 Word의 표기형을 보여준다.
    /// ([`docs/adr/0011`](../../docs/adr/0011-card-front-is-determined-by-sense-kind.md))
    pub fn front<'a>(&'a self, word_lemma: &'a str) -> &'a str {
        match self.kind {
            SenseKind::Example => self.example_en.as_deref().unwrap_or(word_lemma),
            SenseKind::Word | SenseKind::Phrase => word_lemma,
        }
    }

    pub fn is_user_authored(&self) -> bool {
        self.source == SenseSource::User
    }
}

/// 외부 사전에서 가져와 저장할 단어.
///
/// **전역이라 모든 사용자에게 보인다.** 사용자 단어와 `source` 로 구분된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertDictionaryWord {
    pub lemma: String,
    pub phonetic: Option<String>,
    /// 비어 있으면 거절한다. 뜻 없는 단어를 넣으면 큐에서 아무것도 안 나온다.
    pub senses: Vec<DictionarySense>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionarySense {
    pub kind: SenseKind,
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
}

/// 학습자가 직접 지은 Sense.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUserSense {
    pub lemma: String,
    /// `phrase`면 이 값이 새 Word의 표기형이 되고, 그 밑에 Sense가 붙는다.
    pub kind: SenseKind,
    /// `example`이면 필수 — Card 앞면이 여기서 나온다.
    pub example_en: Option<String>,
    pub definition: String,
    pub pos: Option<String>,
}

impl NewUserSense {
    /// 도메인 규칙을 여기서 거른다. 저장소가 더하는 것보다 앞에서 막는 편이
    /// 메시지가 정확하다.
    pub fn validate(&self) -> crate::StoreResult<()> {
        use crate::error::invalid;

        if self.lemma.trim().is_empty() {
            return Err(invalid("lemma이 비었다"));
        }
        if self.definition.trim().is_empty() {
            return Err(invalid("definition이 비었다"));
        }
        if self.kind == SenseKind::Example
            && self.example_en.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(invalid("example kind는 example_en이 필요하다"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreError;

    fn id(n: u8) -> WordId {
        let mut uuid = [0u8; 16];
        uuid[15] = n;
        Id::from(uuid)
    }

    fn sense(kind: SenseKind, example_en: Option<&str>) -> SenseView {
        SenseView {
            id: id(1),
            word_id: id(0),
            kind,
            source: SenseSource::User,
            pos: None,
            definition: "포기하다".into(),
            example_en: example_en.map(str::to_owned),
            example_ko: None,
            in_decks: Vec::new(),
        }
    }

    #[test]
    fn word_and_phrase_show_the_lemma() {
        let lemma = "abandon";
        assert_eq!(sense(SenseKind::Word, None).front(lemma), "abandon");
        assert_eq!(
            sense(SenseKind::Phrase, Some("run off")).front(lemma),
            "abandon"
        );
    }

    #[test]
    fn example_shows_the_sentence_not_the_lemma() {
        let sentence = "They abandoned the car and fled.";
        assert_eq!(
            sense(SenseKind::Example, Some(sentence)).front("abandon"),
            sentence
        );
    }

    #[test]
    fn example_without_a_sentence_falls_back_to_the_lemma() {
        // 방어적 분기. validate()가 막지만, 사전에서 들어온 데이터에는 예문이
        // 없을 수 있다. 앞면이 비어 보이지 않게 한다.
        assert_eq!(sense(SenseKind::Example, None).front("abandon"), "abandon");
    }

    #[test]
    fn new_sense_rejects_an_empty_definition() {
        let input = NewUserSense {
            lemma: "개인단어".into(),
            kind: SenseKind::Word,
            example_en: None,
            definition: "   ".into(),
            pos: None,
        };
        assert!(matches!(input.validate(), Err(StoreError::Invalid(_))));
    }

    #[test]
    fn new_sense_rejects_an_empty_lemma() {
        let input = NewUserSense {
            lemma: "".into(),
            kind: SenseKind::Word,
            example_en: None,
            definition: "뜻".into(),
            pos: None,
        };
        assert!(input.validate().is_err());
    }

    #[test]
    fn example_sense_requires_the_sentence() {
        let input = NewUserSense {
            lemma: "abandon".into(),
            kind: SenseKind::Example,
            example_en: None,
            definition: "이 문장에서는 '떠나다'다".into(),
            pos: None,
        };
        assert!(input.validate().is_err());
    }

    #[test]
    fn a_well_formed_user_sense_passes() {
        let input = NewUserSense {
            lemma: "run off".into(),
            kind: SenseKind::Phrase,
            example_en: Some("He ran off without paying.".to_owned()),
            definition: "도망치다".into(),
            pos: None,
        };
        assert!(input.validate().is_ok());
    }

    #[test]
    fn word_senses_come_back_in_a_stable_order() {
        let mut high = sense(SenseKind::Word, None);
        high.id = id(9);
        let low = sense(SenseKind::Word, None);
        let view = WordView::new(
            id(0),
            "abandon".into(),
            WordSource::User,
            None,
            None,
            vec![high, low.clone()],
        );
        let ids: Vec<_> = view.senses.iter().map(|s| s.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "정렬이 없으면 목록이 매번 흔들린다");
        assert!(view.is_user_authored());
    }
}

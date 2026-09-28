//! 외부 사전 응답 파싱.
//!
//! 순수 함수로 둔다. 네트워크는 [`crate::client`] 가 하고, 형식이 어떻게 생겼든
//! **여기서만 해석한다.** 외부 서비스가 낯선 값을 내려도 서버가 죽지 않아야 한다 —
//! 파싱 실패는 [`ParseOutcome::Skipped`] 다.
//!
//! 근거: [ADR-0007](../../docs/adr/0007-word-data-is-fetched-at-runtime-not-bundled.md) —
//! 단어는 런타임에 가져온다.

use serde::Deserialize;
use voca_domain::{SenseKind, SenseSource, WordSource};

/// 한 뜻.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSense {
    pub kind: SenseKind,
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
}

/// 파싱 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedWord {
    pub lemma: String,
    pub phonetic: Option<String>,
    pub senses: Vec<ParsedSense>,
}

/// 파싱이 어떻게 끝났는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    /// 쓸 만한 뜻이 하나 이상 있다.
    Usable,
    /// 형식은 맞지만 쓸 것이 없다 (빈 뜻 목록, 전부 빈 문자열).
    Empty,
    /// 형식을 못 알아봤다. 원인이 뭐든 **건너뛴다.**
    Unrecognized,
}

impl ParseOutcome {
    pub fn is_usable(self) -> bool {
        self == ParseOutcome::Usable
    }
}

// ── 외부 형식 ────────────────────────────────────────────

/// dictionaryapi.dev 응답의 필요한 부분만.
///
/// **`deny_unknown_fields` 를 세 겹에 건다.** 이게 없으면 필드 이름이 바뀔 때
/// serde 가 조용히 빈 구조체를 만들고, 그 결과 "이 단어에는 뜻이 없다"가 되어
/// **캐시에 영구 저장된다.** 그러면 사용자는 모든 단어에서 "찾을 수 없습니다"를
/// 보게 되고, 원인은 API 필드 이름 하나 바뀐 것뿐이다.
///
/// 조용히 데이터를 잃는 쪽보다 loudly 실패하는 쪽을 택한다. 새 필드가 추가돼
/// 여기서 깨지면 그때 고치면 된다.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiResponse {
    word: Option<String>,
    phonetic: Option<String>,
    #[serde(default)]
    meanings: Vec<ApiMeaning>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiMeaning {
    /// 외부 필드명이 camelCase 다. 그대로 두고 serde 로만 매핑한다 — 변수 이름을
    /// 우리 관용에 맞추면 형식이 바뀔 때 조용히 놓친다.
    #[serde(default, rename = "partOfSpeech")]
    part_of_speech: Option<String>,
    #[serde(default)]
    definitions: Vec<ApiDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiDefinition {
    #[serde(default)]
    definition: Option<String>,
    #[serde(default)]
    example: Option<String>,
}

/// 응답 JSON 을 파싱한다.
///
/// **최상위가 배열일 수 있다.** 실제 dictionaryapi.dev 는 어형별로 항목 하나씩
/// 나눠서 배열로 돌려준다 (`run` 은 항목이 여럿이다). 단일 객체로만 읽으면 실제로는
/// 존재하는 단어를 전부 `Unrecognized` 로 돌려주는데, 그건 사용자에게는
/// "단어를 찾을 수 없습니다" 로 보인다.
///
/// 항목이 여럿이면 **뜻을 전부 합친다.** 하나만 고르면 그건 임의의 선택이고,
/// 어형 하나의 뜻만 외우게 된다.
pub fn parse(body: &str) -> (ParseOutcome, Option<ParsedWord>) {
    let entries: Vec<ApiResponse> = match serde_json::from_str::<Vec<ApiResponse>>(body) {
        Ok(v) => v,
        Err(_) => match serde_json::from_str::<ApiResponse>(body) {
            Ok(v) => vec![v],
            Err(_) => return (ParseOutcome::Unrecognized, None),
        },
    };

    // 표기형과 발음은 첫 항목에서 가져온다. 뒤 항목이 같은 단어의 다른 표기일 수 있다.
    let lemma = entries
        .iter()
        .filter_map(|e| e.word.as_deref())
        .map(str::trim)
        .find(|w| !w.is_empty())
        .map(str::to_owned);

    let phonetic = entries
        .iter()
        .filter_map(|e| e.phonetic.as_deref())
        .map(str::trim)
        .find(|p| !p.is_empty())
        .map(str::to_owned);

    let mut senses: Vec<ParsedSense> = Vec::new();
    for api in &entries {
        for meaning in &api.meanings {
            let pos = meaning
                .part_of_speech
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned);

            for definition in &meaning.definitions {
                let Some(text) = definition
                    .definition
                    .as_deref()
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                else {
                    // 빈 뜻은 버린다. 이건 흔하다.
                    continue;
                };

                let sense = ParsedSense {
                    // 외부 API 는 관용구를 따로 주지 않는다. 다의어의 각 뜻이 이
                    // 앱에서는 곧 하나의 Sense 다.
                    kind: SenseKind::Word,
                    pos: pos.clone(),
                    definition: text.to_owned(),
                    example_en: definition
                        .example
                        .as_deref()
                        .map(str::trim)
                        .filter(|e| !e.is_empty())
                        .map(str::to_owned),
                    example_ko: None,
                };

                // 항목이 겹치면 같은 뜻이 두 번 들어온다. 뜻풀이 문장이 같으면 하나로
                // 본다 — 같은 Sense 를 두 번 만들면 큐에 두 번 나온다.
                let duplicate = senses
                    .iter()
                    .any(|s| s.definition == sense.definition && s.pos == sense.pos);
                if !duplicate {
                    senses.push(sense);
                }
            }
        }
    }

    if lemma.is_none() || senses.is_empty() {
        return (ParseOutcome::Empty, None);
    }

    (
        ParseOutcome::Usable,
        Some(ParsedWord {
            lemma: lemma.expect("위에서 걸렀다"),
            phonetic,
            senses,
        }),
    )
}

/// 파싱 결과를 도메인 행으로 옮긴다.
pub fn to_domain(word: &ParsedWord) -> (WordSource, Vec<(SenseKind, SenseSource, ParsedSense)>) {
    (
        WordSource::Dictionary,
        word.senses
            .iter()
            .map(|s| (s.kind, SenseSource::Dictionary, s.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_response_yields_every_meaning() {
        let body = r#"{
            "word": "run",
            "phonetic": "/rʌn/",
            "meanings": [
                {"partOfSpeech": "verb",
                 "definitions": [
                    {"definition": "to move fast on foot", "example": "She runs daily."},
                    {"definition": "to manage", "example": "He runs a company."}
                 ]},
                {"partOfSpeech": "noun",
                 "definitions": [{"definition": "an act of running"}]}
            ]
        }"#;
        let (outcome, word) = parse(body);
        assert_eq!(outcome, ParseOutcome::Usable);
        let word = word.unwrap();

        assert_eq!(word.lemma, "run");
        assert_eq!(word.phonetic.as_deref(), Some("/rʌn/"));
        // 다의어는 뜻마다 별개의 Sense 다 (docs/adr/0004).
        assert_eq!(word.senses.len(), 3);
        assert_eq!(word.senses[0].pos.as_deref(), Some("verb"));
        assert_eq!(word.senses[1].definition, "to manage");
        assert_eq!(word.senses[2].pos.as_deref(), Some("noun"));
    }

    #[test]
    fn a_sense_without_an_example_is_still_usable() {
        let body = r#"{"word":"oblige","meanings":[{"partOfSpeech":"verb",
            "definitions":[{"definition":"to do something for someone"}]}]}"#;
        let (outcome, word) = parse(body);
        assert!(outcome.is_usable());
        assert!(word.unwrap().senses[0].example_en.is_none());
    }

    #[test]
    fn a_sense_without_a_part_of_speech_is_still_usable() {
        let body = r#"{"word":"issue","meanings":[{"definitions":[
            {"definition":"a matter for debate"}]}]}"#;
        let (outcome, word) = parse(body);
        assert!(outcome.is_usable());
        assert!(word.unwrap().senses[0].pos.is_none());
    }

    #[test]
    fn a_word_with_no_meanings_is_empty_not_unrecognized() {
        // API 는 404 대신 200 과 빈 목록을 준다. "없다"와 "형식이 바뀌었다"를
        // 구분해야 캐시 정책이 갈린다.
        let (outcome, _) = parse(r#"{"word":"asdfghjkl","meanings":[]}"#);
        assert_eq!(outcome, ParseOutcome::Empty);
    }

    #[test]
    fn malformed_json_is_skipped_rather_than_fatal() {
        // 외부 서비스가 뭐라도 내려도 서버는 죽지 않는다.
        for body in [
            "",
            "not json",
            "null",
            r#"{"word":123}"#,
            r#"[{"vocabulary":"x"}]"#,
        ] {
            let (outcome, word) = parse(body);
            assert_eq!(outcome, ParseOutcome::Unrecognized, "body={body:?}");
            assert!(word.is_none());
        }
    }

    #[test]
    fn an_empty_array_is_not_found_not_unrecognized() {
        // `[]` 는 **형식이 맞고 내용이 비어 있다.** 필드를 못 읽은 게 아니므로
        // 조용히 넘어가지도 않고 경고도 남기지 않는다 — "이 사전에 없다" 다.
        for body in ["[]", r#"[]"#] {
            let (outcome, word) = parse(body);
            assert_eq!(outcome, ParseOutcome::Empty, "body={body:?}");
            assert!(word.is_none());
        }
    }

    #[test]
    fn a_real_array_of_entries_is_parsed() {
        // 실제로 dictionaryapi.dev 가 돌려주는 모양. 항목이 둘이다.
        let body = r#"[
            {"word":"run","phonetic":"/ɹʌn/","meanings":[
                {"partOfSpeech":"verb","definitions":[
                    {"definition":"To move fast.","example":"She runs."}
                ]}
            ]},
            {"word":"Runs","meanings":[
                {"partOfSpeech":"noun","definitions":[
                    {"definition":"Uncovered ground."}
                ]}
            ]}
        ]"#;
        let (outcome, word) = parse(body);
        assert_eq!(outcome, ParseOutcome::Usable);
        let word = word.expect("배열 응답에서 단어를 못 읽었다");
        assert_eq!(word.lemma, "run");
        assert_eq!(word.phonetic.as_deref(), Some("/ɹʌn/"));
        // 항목이 둘이어도 뜻은 전부 온다 — 하나만 고르면 임의의 선택이 된다.
        assert_eq!(word.senses.len(), 2, "{:?}", word.senses);
        assert_eq!(word.senses[1].pos.as_deref(), Some("noun"));
    }

    #[test]
    fn a_repeated_meaning_across_entries_appears_once() {
        // 항목이 겹치면 같은 뜻이 두 번 들어온다. 그대로 두면 큐에 두 번 나온다.
        let body = r#"[
            {"word":"run","meanings":[{"partOfSpeech":"verb","definitions":[
                {"definition":"To move fast."}
            ]}]},
            {"word":"run","meanings":[{"partOfSpeech":"verb","definitions":[
                {"definition":"To move fast."}
            ]}]}
        ]"#;
        let (_, word) = parse(body);
        assert_eq!(word.expect("없음").senses.len(), 1);
    }

    #[test]
    fn a_response_with_a_different_shape_is_skipped_loudly() {
        // 필드 이름이 바뀌면 **Unrecognized** 여야 한다. Empty 가 되면 "뜻이 없다"가
        // 되어 캐시에 저장되고, 사용자는 모든 단어에서 "찾을 수 없습니다"를 본다.
        let (outcome, word) = parse(r#"{"data":[{"word":"run"}]}"#);
        assert_eq!(outcome, ParseOutcome::Unrecognized);
        assert!(word.is_none());

        // 중첩 구조가 바뀌어도 같다.
        assert_eq!(
            parse(r#"{"word":"run","meanings":[{"sense":[{"definition":"x"}]}]}"#).0,
            ParseOutcome::Unrecognized
        );
    }

    #[test]
    fn an_added_field_also_stops_parsing_loudly() {
        // 새 필드가 추가돼도 조용히 넘어가면 그 필드의 데이터를 잃는다. 그것보다
        // 여기서 멈추는 게 낫다 — 서버 로그에 "형식이 바뀌었다"가 남는다.
        assert_eq!(
            parse(r#"{"word":"run","meanings":[],"newField":1}"#).0,
            ParseOutcome::Unrecognized
        );
    }

    #[test]
    fn blank_definitions_are_dropped() {
        let body = r#"{"word":"run","meanings":[{"partOfSpeech":"verb","definitions":[
            {"definition":"  "},
            {"definition":""},
            {"definition":"  to move fast  "}
        ]}]}"#;
        let (_, word) = parse(body);
        let senses = word.unwrap().senses;
        assert_eq!(senses.len(), 1, "빈 뜻이 통과했다");
        assert_eq!(senses[0].definition, "to move fast", "공백이 남았다");
    }

    #[test]
    fn a_blank_word_is_not_a_word() {
        let (outcome, _) =
            parse(r#"{"word":"   ","meanings":[{"definitions":[{"definition":"x"}]}]}"#);
        assert_eq!(outcome, ParseOutcome::Empty);
    }

    #[test]
    fn a_phrase_comes_back_as_a_word_sense_until_we_know_more() {
        // 외부 API 는 관용구를 따로 구분해 주지 않는다. 일단 다의어의 뜻으로 넣고,
        // 사용자가 `phrase` 로 바꿀 수 있게 한다.
        let body = r#"{"word":"run off","meanings":[{"partOfSpeech":"phrasal verb",
            "definitions":[{"definition":"to escape"}]}]}"#;
        let (outcome, word) = parse(body);
        assert!(outcome.is_usable());
        let senses = word.unwrap().senses;
        assert_eq!(senses[0].kind, SenseKind::Word);
        assert_eq!(senses[0].pos.as_deref(), Some("phrasal verb"));
    }

    #[test]
    fn the_domain_conversion_marks_everything_as_dictionary() {
        let body = r#"{"word":"run","meanings":[{"partOfSpeech":"verb",
            "definitions":[{"definition":"to move"}]}]}"#;
        let (_, word) = parse(body);
        let word = word.unwrap();
        let (source, senses) = to_domain(&word);
        assert_eq!(source, WordSource::Dictionary);
        assert_eq!(senses.len(), 1);
        assert_eq!(senses[0].1, SenseSource::Dictionary);
    }
}

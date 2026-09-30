//! 외부 사전 클라이언트.
//!
//! 순서: **캐시 먼저 → 없으면 네트워크 → 결과를 캐시에 넣고 저장.**
//!
//! 왜 순서가 이렇냐:
//! - 캐시 우선이어야 같은 단어를 두 번째로 추가할 때 기다리지 않는다
//! - 결과를 저장해야 세 번째부터는 네트워크를 전혀 보지 않는다
//!
//! 네트워크가 죽어도 **이미 캐시된 단어는 계속 보인다.** 사전 조회는 앱의 부가
//! 기능이지 전제가 아니다.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use serde::Deserialize;
use tokio::time::timeout;
use voca_store::UpsertDictionaryWord;
use voca_store_sqlite::{FetchStatus, SqliteStore};

use crate::parse::{self, ParseOutcome};

/// 이 조회에 걸어 둔 시간.
///
/// 외부 서비스가 느려도 우리 요청이 먼저 끊긴다. 사용자는 8초를 기다리다 포기하는
/// 것보다 4초를 기다리고 "지금은 안 돼"를 보는 편이 낫다.
const FETCH_TIMEOUT: StdDuration = StdDuration::from_secs(4);

/// 실패한 조회를 캐시에 얼마나 둘지.
const FAILURE_TTL_SECONDS: i64 = 10 * 60;

/// 단어를 조회한 결과.
///
/// **저장까지 마친 뒤** 돌려준다. 호출부가 받아서 또 저장할 일이 없게 하는 편이
/// simplicity 에 낫다.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    /// 캐시에서 찾아 저장된 것을 돌려준다. 네트워크를 보지 않았다.
    FromCache(Box<voca_store::WordView>),
    /// 네트워크에서 가져와 저장했다.
    Fetched(Box<voca_store::WordView>),
    /// 뜻은 얻었지만 **저장에 실패했다.** 화면에는 보여줄 수 있다.
    ///
    /// 별도 상태로 두는 이유: 화면에 보여줄 `WordView` 를 만들려면 식별자가 필요한데,
    /// 저장이 안 됐다면 그게 없다. 가짜 식별자를 만들어 넣으면 나중에 그 값이 진짜로
    /// 쓰여서 다른 Card 를 가리킬 수 있다. **없는 식별자를 지어내지 않는다.**
    FetchedUnstored(Box<UpsertDictionaryWord>),
    /// 이 사전에 없는 단어다.
    NotFound,
    /// 외부 서비스가 실패했다. **캐시에 있는 이전 값이 있으면 그것을 돌려준다.**
    Unavailable,
    /// 응답은 왔지만 형식을 알아볼 수 없었다.
    ///
    /// **이건 조용히 넘기지 않는다.** 필드 이름이 바뀌면 이 결과가 계속 나온다.
    /// 그때 사용자는 "단어를 찾을 수 없습니다"를 보고, 원인은 API 변경 하나다.
    Unrecognized,
}

impl Lookup {
    /// 저장까지 끝난 결과인가.
    pub fn is_stored(&self) -> bool {
        matches!(self, Lookup::FromCache(_) | Lookup::Fetched(_))
    }

    /// 뜻이 있다가 (저장 여부와 무관하게).
    pub fn has_meanings(&self) -> bool {
        self.is_stored() || matches!(self, Lookup::FetchedUnstored(_))
    }

    /// 네트워크를 보았는가.
    pub fn touched_network(&self) -> bool {
        !matches!(self, Lookup::FromCache(_))
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Lookup::NotFound)
    }
}

/// 사전 조회기.
pub struct Dictionary {
    client: reqwest::Client,
    /// 서버는 `AppState` 안에 이 값과 같은 `Arc` 를 이미 들고 있다. 풀을 두 개
    /// 만들면 캐시 읽기가 서로를 못 본다 — 그래서 **공유를 기본값으로 삼는다.**
    store: Arc<SqliteStore>,
    endpoint: String,
}

impl Dictionary {
    /// `SqliteStore` 와 `Arc<SqliteStore>` 를 모두 받는다.
    pub fn new(store: impl Into<Arc<SqliteStore>>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("voca-memory/", env!("CARGO_PKG_VERSION")))
            // 우리 타임아웃이 더 짧다. 두 개를 같이 두면 헷갈린다.
            .connect_timeout(FETCH_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "HTTP 클라이언트를 만들지 못했다");
                reqwest::Client::new()
            });

        Self {
            client,
            store: store.into(),
            endpoint: "https://api.dictionaryapi.dev/api/v2/entries/en/".to_owned(),
        }
    }

    /// 테스트와 대안 출처를 위한 끝점 교체.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into().trim_end_matches('/').to_owned() + "/";
        self
    }

    /// 단어나 문장의 미국식 원어민 TTS 오디오 스트림(MP3)을 가져온다.
    pub async fn fetch_tts_audio(&self, text: &str) -> Option<Vec<u8>> {
        let text = text.trim();
        if text.is_empty() || text.len() > 300 {
            return None;
        }

        let encoded = url_escape_query(text);
        let url = format!(
            "https://translate.google.com/translate_tts?ie=UTF-8&tl=en&client=tw-ob&q={encoded}"
        );

        match timeout(FETCH_TIMEOUT, self.client.get(&url).send()).await {
            Ok(Ok(resp)) if resp.status().is_success() => {
                match resp.bytes().await {
                    Ok(bytes) if !bytes.is_empty() => Some(bytes.to_vec()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// 표기형의 뜻들을 찾아 온다.
    pub async fn lookup(&self, lemma: &str) -> Lookup {
        let lemma = lemma.trim();
        if lemma.is_empty() {
            return Lookup::NotFound;
        }

        // 1. 캐시
        if let Ok(Some((body, status))) = self.store.dict_cache_get(lemma).await {
            match self.stored(lemma, &body).await {
                Lookup::Unrecognized => {
                    // 캐시된 응답을 못 읽는다. 네트워크로 다시 받아 본다 — 새 형식일
                    // 수도 있다.
                    tracing::warn!(lemma, "캐시된 사전 응답을 읽지 못했다. 다시 가져온다");
                }
                other => {
                    if status == FetchStatus::NotFound && matches!(other, Lookup::NotFound) {
                        return Lookup::NotFound;
                    }
                    // 여기서 돌려주는 것은 **네트워크를 보지 않은 결과**다. `stored()`
                    // 는 저장을 마치고 `Fetched` 로 돌려주므로, 그대로 넘기면 두 번
                    // 조회한 것과 한 번 조회한 것이 구별되지 않는다 — 화면은 캐시가
                    // 된 걸 모르고 매번 "새로 가져옴"을 보여 준다.
                    return match other {
                        Lookup::Fetched(w) => Lookup::FromCache(w),
                        // 저장은 다시 실패했다. 출처가 캐시든 네트워크든 사용자가 볼
                        // 답은 같다: 뜻은 있는데 추가할 수 없다.
                        other => other,
                    };
                }
            }
        }

        // 2. 이미 로컬 DB에 등록된 사전 단어인지 확인 (시드 단어 또는 이전 저장 단어)
        if let Ok(Some(word_view)) = self.store.find_dictionary_word_by_lemma(lemma).await {
            return Lookup::FromCache(Box::new(word_view));
        }

        // 3. 네트워크
        let url = format!("{}{}", self.endpoint, url_escape(lemma));
        let fetched = match timeout(FETCH_TIMEOUT, self.client.get(&url).send()).await {
            Ok(Ok(response)) => {
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                if status == 404 {
                    let _ = self
                        .store
                        .dict_cache_put(lemma, "[]", FetchStatus::NotFound, FAILURE_TTL_SECONDS)
                        .await;
                    return Lookup::NotFound;
                }
                if !(200..300).contains(&status) {
                    tracing::warn!(lemma, status, "사전 API 가 오류를 돌려주었다");
                    let _ = self
                        .store
                        .dict_cache_put(lemma, "", FetchStatus::Error, FAILURE_TTL_SECONDS)
                        .await;
                    if let Some(fallback) = self.try_datamuse_fallback(lemma).await {
                        return fallback;
                    }
                    return Lookup::Unavailable;
                }
                if let Err(e) = self
                    .store
                    .dict_cache_put(lemma, &body, FetchStatus::Ok, FAILURE_TTL_SECONDS)
                    .await
                {
                    // 캐시에 못 넣어도 조회는 성공했다. 조회 결과를 돌려준다.
                    tracing::warn!(lemma, error = %e, "사전 응답을 캐시하지 못했다");
                }
                body
            }
            Ok(Err(e)) => {
                tracing::warn!(lemma, error = %e, "사전 API 요청이 실패했다");
                let _ = self
                    .store
                    .dict_cache_put(lemma, "", FetchStatus::Error, FAILURE_TTL_SECONDS)
                    .await;
                if let Some(fallback) = self.try_datamuse_fallback(lemma).await {
                    return fallback;
                }
                return Lookup::Unavailable;
            }
            Err(_) => {
                // 우리가 먼저 끊었다. 원인은 저쪽 지연이거나 ours 지연이다 — 밖에서
                // 알 수 없다. 새 요청을 시도하지 않는다. 너무 느린 API 를 반복 호출하면
                // 그것도 DoS 다.
                tracing::warn!(lemma, "사전 API 응답이 시간 내에 오지 않았다");
                if let Some(fallback) = self.try_datamuse_fallback(lemma).await {
                    return fallback;
                }
                return Lookup::Unavailable;
            }
        };

        self.stored(lemma, &fetched).await
    }

    /// 본문을 파싱해 저장까지 마친다.
    async fn stored(&self, lemma: &str, body: &str) -> Lookup {
        let (outcome, parsed) = parse::parse(body);
        match outcome {
            ParseOutcome::Unrecognized => Lookup::Unrecognized,
            ParseOutcome::Empty => Lookup::NotFound,
            ParseOutcome::Usable => {
                let Some(word) = parsed else {
                    return Lookup::Unrecognized;
                };
                let (source, senses) = parse::to_domain(&word);
                let input = UpsertDictionaryWord {
                    lemma: word.lemma,
                    phonetic: word.phonetic,
                    senses: senses
                        .into_iter()
                        .map(|(_, _, s)| voca_store::DictionarySense {
                            kind: s.kind,
                            pos: s.pos,
                            definition: s.definition,
                            example_en: s.example_en,
                            example_ko: s.example_ko,
                        })
                        .collect(),
                };
                let _ = source;

                match self.store.upsert_dictionary_word(input.clone()).await {
                    Ok(view) => Lookup::Fetched(Box::new(view)),
                    Err(e) => {
                        // 저장은 실패했지만 조회는 성공이다. 사용자에게는 뜻이 보인다.
                        tracing::warn!(error = ?e, lemma, "사전 단어를 저장하지 못했다");
                        Lookup::FetchedUnstored(Box::new(input))
                    }
                }
            }
        }
    }

    /// 기본 사전 API가 일시 장애일 때 Datamuse API에서 뜻을 가져온다.
    async fn try_datamuse_fallback(&self, lemma: &str) -> Option<Lookup> {
        if self.endpoint != "https://api.dictionaryapi.dev/api/v2/entries/en/" {
            return None;
        }

        let url = format!(
            "https://api.datamuse.com/words?sp={}&md=d&max=1",
            url_escape(lemma)
        );
        let res = match timeout(FETCH_TIMEOUT, self.client.get(&url).send()).await {
            Ok(Ok(r)) if r.status().is_success() => r,
            _ => return None,
        };

        let items: Vec<DatamuseItem> = res.json().await.ok()?;
        let item = items
            .into_iter()
            .find(|i| i.word.eq_ignore_ascii_case(lemma))?;

        if item.defs.is_empty() {
            return None;
        }

        let mut senses = Vec::new();
        for d in item.defs {
            let (pos_str, def_str) = match d.split_once('\t') {
                Some((p, rest)) => (p.trim(), rest.trim()),
                None => ("", d.trim()),
            };
            if def_str.is_empty() {
                continue;
            }
            let pos = match pos_str {
                "n" => Some("noun".to_owned()),
                "v" => Some("verb".to_owned()),
                "adj" => Some("adjective".to_owned()),
                "adv" => Some("adverb".to_owned()),
                "" => None,
                other => Some(other.to_owned()),
            };
            senses.push(voca_store::DictionarySense {
                kind: voca_domain::SenseKind::Word,
                pos,
                definition: def_str.to_owned(),
                example_en: None,
                example_ko: None,
            });
        }

        if senses.is_empty() {
            return None;
        }

        let input = UpsertDictionaryWord {
            lemma: item.word,
            phonetic: None,
            senses,
        };

        match self.store.upsert_dictionary_word(input.clone()).await {
            Ok(view) => Some(Lookup::Fetched(Box::new(view))),
            Err(e) => {
                tracing::warn!(error = ?e, lemma, "Datamuse 사전 단어를 저장하지 못했다");
                Some(Lookup::FetchedUnstored(Box::new(input)))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct DatamuseItem {
    word: String,
    #[serde(default)]
    defs: Vec<String>,
}

/// 표기형을 URL 조각으로 바꾼다.
///
/// `encode` 를 그대로 쓰면 공백이 `%20` 이 되고, 어댑터는 붙는 일이 별로 없어
/// 어설픈 처리가 생긴다. 우리가 아는 형태(소문자, 공백, 하이픈)만 허용하고
/// 나머지는 조용히 지운다 — 우리가 만드는 URL 이므로 안전하다.
fn url_escape(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '\'' | '.'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect::<String>()
}

/// 쿼리 파라미터용 URL 인코딩. 공백을 '+'로 바꾸고 특수 문자를 퍼센트 인코딩한다.
fn url_escape_query(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for c in raw.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push('+'),
            other => {
                let mut buf = [0u8; 4];
                let s = other.encode_utf8(&mut buf);
                for b in s.as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_becomes_a_dash_separated_path() {
        assert_eq!(url_escape("run off"), "run-off");
        assert_eq!(url_escape("Don't"), "Don't");
        assert_eq!(url_escape("well-known"), "well-known");
    }

    #[test]
    fn path_separators_cannot_escape_the_endpoint() {
        // 이게 전부다. 조립된 URL 이 다른 출처를 가리키면 안 된다.
        assert_eq!(url_escape("../../etc/passwd"), "....etcpasswd");
        assert_eq!(url_escape("a/b?c=d"), "abcd");
        assert_eq!(url_escape("a#b"), "ab");
    }

    #[test]
    fn symbols_are_dropped_rather_than_encoded() {
        // 조립한 URL 이 다른 출처를 가리키면 안 된다. 허용하지 않은 문자는 지운다.
        // 공백이 대시로 바뀌는 것도 그一례이다.
        assert_eq!(url_escape("?!@#%^&*()"), "");
        assert_eq!(url_escape("   "), "---");
        assert_eq!(url_escape("a b"), "a-b");
    }

    #[tokio::test]
    async fn a_blank_lookup_never_reaches_the_network() {
        // 공백뿐인 입력이 대시 세 개가 된 URL 로 나가지 않는다. `lookup` 이 먼저
        // 걸러야 한다. 포트가 1 이라 그쪽으로 나갔다면 즉시 실패했을 것이다.
        let dir = tempfile::TempDir::new().unwrap();
        let store = voca_store_sqlite::SqliteStore::open(&dir.path().join("v.db"))
            .await
            .unwrap();
        let dict = Dictionary::new(store).with_endpoint("http://127.0.0.1:1/never");

        assert!(dict.lookup("   ").await.is_not_found());
        assert!(dict.lookup("").await.is_not_found());
    }

    #[tokio::test]
    async fn a_word_already_in_store_is_served_without_network() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = voca_store_sqlite::SqliteStore::open(&dir.path().join("v.db"))
            .await
            .unwrap();

        store
            .upsert_dictionary_word(UpsertDictionaryWord {
                lemma: "run".to_owned(),
                phonetic: Some("/rʌn/".to_owned()),
                senses: vec![voca_store::DictionarySense {
                    kind: voca_domain::SenseKind::Word,
                    pos: Some("verb".to_owned()),
                    definition: "To move quickly".to_owned(),
                    example_en: None,
                    example_ko: None,
                }],
            })
            .await
            .unwrap();

        let dict = Dictionary::new(store).with_endpoint("http://127.0.0.1:1/never");
        let result = dict.lookup("run").await;
        match result {
            Lookup::FromCache(w) => {
                assert_eq!(w.lemma, "run");
                assert_eq!(w.senses[0].definition, "To move quickly");
            }
            other => panic!("스토어에 있는 단어는 FromCache 로 와야 한다: {:?}", other),
        }
    }
}

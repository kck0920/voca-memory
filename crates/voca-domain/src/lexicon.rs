use serde::{Deserialize, Serialize};

use crate::Id;

/// 어휘 항목. `run`이 Word다.
///
/// 모든 Word는 하나 이상의 Sense를 가지며, 모든 Sense는 정확히 하나의 Word에 속한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub id: Id,
    pub lemma: String,
    pub source: WordSource,
    pub phonetic: Option<String>,
    /// 재생 전용 음성 자산의 위치. Word에 붙고 Sense에는 붙지 않는다.
    pub audio_url: Option<String>,
}

/// Word가 어디서 왔는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordSource {
    /// 외부 사전에서 온 전역 행. 앱이 직접 쓰지 않는다.
    Dictionary,
    /// 학습자가 직접 지은 단어.
    User,
}

/// Word가 가지는 하나의 뜻 단위.
///
/// 사전의 뜻, 관용구, 특정 문맥에서의 쓰임, 학습자가 직접 지은 뜻이 모두 Sense다.
/// 복습 단위를 쪼갤 때의 기준이며, Card는 정확히 하나의 Sense를 향한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sense {
    pub id: Id,
    pub word_id: Id,
    pub kind: SenseKind,
    pub source: SenseSource,
    /// 품사. 관용구·문맥 예문에는 없을 수 있다.
    pub pos: Option<String>,
    pub definition: String,
    pub example_en: Option<String>,
    pub example_ko: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseKind {
    /// 사전의 개별 뜻. "run (v.) 달리다"
    Word,
    /// 관용구·파생구문. "run off (도망치다)"
    Phrase,
    /// 특정 문맥에서의 쓰임을 익히는 카드. "이 문장에서는 뜻이 다르다"
    Example,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseSource {
    Dictionary,
    User,
}

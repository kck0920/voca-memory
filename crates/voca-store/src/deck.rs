use serde::{Deserialize, Serialize};
use voca_domain::Id;

use crate::{CardId, SenseId};

/// 덱을 화면에 넘길 때 필요한 것.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckView {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    /// 하루 목표 복습 건수. 기본값 20.
    pub daily_goal: u32,
    /// 하루에 새로 배우는 Card 수.
    ///
    /// 복습과 신규는 서로 다른 자원을 쓴다 — 하루 목표가 20이어도 신규 100개를
    /// 한꺼번에 꺼내면 안 된다. Anki와 같은 구조.
    pub new_per_day: u32,
    pub revision: crate::Revision,
}

impl DeckView {
    /// 하루 목표가 줄었을 때 신규 한도도 따라줄까?
    ///
    /// 안 따른다. 두 값은 사용자가 의도적으로 다르게 설정하는 것이다.
    /// `daily_goal`은 "오늘 몇 개나 할 것인가", `new_per_day`는 "하루에 몇 개를
    /// 새로 배울 것인가"다.
    pub fn is_night_heavy(&self) -> bool {
        self.new_per_day > self.daily_goal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDeck {
    pub name: String,
    pub description: Option<String>,
    pub daily_goal: Option<u32>,
    pub new_per_day: Option<u32>,
}

impl NewDeck {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            daily_goal: None,
            new_per_day: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckUpdate {
    pub id: Id,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub daily_goal: Option<u32>,
    pub new_per_day: Option<u32>,
}

/// 덱에 넣을 Sense들.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddCards {
    pub deck_id: Id,
    pub senses: Vec<SenseId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardView {
    pub id: CardId,
    pub deck_id: Id,
    pub sense_id: SenseId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(daily_goal: u32, new_per_day: u32) -> DeckView {
        DeckView {
            id: Id::from([0u8; 16]),
            name: "수능 단어".into(),
            description: None,
            daily_goal,
            new_per_day,
            revision: crate::Revision::initial(),
        }
    }

    #[test]
    fn the_two_daily_limits_are_independent() {
        let d = deck(20, 10);
        assert!(!d.is_night_heavy());

        let d = deck(20, 40);
        assert!(
            d.is_night_heavy(),
            "하루 목표 20인데 신규 40이면 스케줄러가 목표를 무시하게 된다"
        );
    }

    #[test]
    fn a_named_deck_falls_back_to_defaults() {
        let d = NewDeck::named("TOEFL 핵심");
        assert_eq!(d.name, "TOEFL 핵심");
        assert_eq!(d.daily_goal, None);
        assert_eq!(d.new_per_day, None);
    }
}

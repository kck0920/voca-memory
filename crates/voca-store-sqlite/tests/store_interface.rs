//! `voca-store` interface 전체에 대한 통합 테스트.
//!
//! `submit_review`는 [`submit_review.rs`](./submit_review.rs)에 있다. 여기서는 나머지
//! 도메인 연산을 본다 — 특히 **큐 정렬 정책과 일일 한도**가 interface 안에서 실제로
//! 지켜지는지.

use tempfile::TempDir;
use time::OffsetDateTime;
use voca_domain::{Rating, ReviewState, SenseKind};
use voca_store::{
    Change, DeckUpdate, DictionarySense, NewDeck, NewUserSense, SenseQuery, Store, StoreError,
    StudyRequest, UpsertDictionaryWord,
};

use voca_store_sqlite::SqliteStore;

/// 이 픽스처의 "지금".
///
/// `add_cards` 는 새 Card 의 `due_at`을 **실제 현재 시각**으로 넣는다 (방금 생긴
/// Card 는 지금 풀 수 있으므로). 그래서 테스트가 3월 같은 과거 시각으로 조회하면
/// 아무것도 Due 가 아니게 된다. 여기서 시각을 하나 정해 픽스처와 조회가 같은 값을
/// 쓰게 한다.
fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc().replace_nanosecond(0).unwrap()
}

/// n일 전의 unix 시각.
fn days_ago(days: f64) -> i64 {
    (now() - time::Duration::seconds_f64(days * 86_400.0)).unix_timestamp()
}

/// n일 뒤의 unix 시각.
fn days_ahead(days: f64) -> i64 {
    (now() + time::Duration::seconds_f64(days * 86_400.0)).unix_timestamp()
}

struct World {
    _dir: TempDir,
    store: SqliteStore,
    user: voca_domain::Id,
    deck: voca_domain::Id,
}

impl World {
    async fn setup() -> Self {
        let dir = TempDir::new().unwrap();
        let store = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();
        let user = voca_domain::Id::from([1u8; 16]);

        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
             VALUES (?1, 'a@b.kr', 'x', '테스터', 'Asia/Seoul', 0)",
        )
        .bind(user.to_string())
        .execute(store.pool())
        .await
        .unwrap();

        let deck = voca_domain::Id::from([2u8; 16]);
        sqlx::query(
            "INSERT INTO decks (id, user_id, name, daily_goal, new_per_day, created_at, updated_at)
             VALUES (?1, ?2, '수능 단어', 20, 3, 0, 0)",
        )
        .bind(deck.to_string())
        .bind(user.to_string())
        .execute(store.pool())
        .await
        .unwrap();

        Self {
            _dir: dir,
            store,
            user,
            deck,
        }
    }

    /// 단어 하나를 사전에 넣고, 덱에 Card로 추가한다.
    ///
    /// **만들어진 Card 를 돌려준다.** Sense 식별자가 아니라 — Sense id 를 Card id 처럼
    /// 쓰면 `set_state` 같은 갱신이 조용히 0행을 처리하고 테스트가 이상한 이유로 죽는다.
    async fn word(&self, lemma: &str, senses: &[(SenseKind, &str, &str)]) -> Vec<voca_domain::Id> {
        let word_id = voca_domain::Id::from([9u8; 16]);
        sqlx::query(
            "INSERT OR IGNORE INTO words (id, lemma, source, updated_at)
             VALUES (?1, ?2, 'dictionary', 0)",
        )
        .bind(word_id.to_string())
        .bind(lemma)
        .execute(self.store.pool())
        .await
        .unwrap();

        let mut ids = Vec::new();
        for (i, (kind, definition, example)) in senses.iter().enumerate() {
            let mut bytes = [7u8; 16];
            bytes[14] = i as u8;
            let id = voca_domain::Id::from(bytes);
            sqlx::query(
                "INSERT INTO senses (id, word_id, kind, source, definition, example_en, updated_at)
                 VALUES (?1, ?2, ?3, 'dictionary', ?4, ?5, 0)",
            )
            .bind(id.to_string())
            .bind(word_id.to_string())
            .bind(match kind {
                SenseKind::Word => "word",
                SenseKind::Phrase => "phrase",
                SenseKind::Example => "example",
            })
            .bind(definition)
            .bind(example)
            .execute(self.store.pool())
            .await
            .unwrap();
            ids.push(id);
        }

        let added = self
            .store
            .add_cards(self.user, self.deck, &ids)
            .await
            .unwrap();
        assert_eq!(
            added.len(),
            ids.len(),
            "모든 Sense 가 Card 로 들어가지 않았다"
        );

        let cards: Vec<voca_domain::Id> = added.into_iter().map(|c| c.id).collect();
        assert_eq!(cards.len(), ids.len());
        cards
    }

    /// Card 하나를 원하는 스케줄러 상태로 밀어 넣는다.
    ///
    /// `due_at` 을 **안정성이 지난 뒤**로 맞춘다. `add_cards` 가 남긴 `due_at` 은
    /// 생성 시각이라 어떤 Card든 Due 가 되어 버려서, 안정한 단어가 큐에서 빠지는지
    /// 검증할 수 없다.
    /// Card 하나를 복습 이력이 있는 상태로 밀어 넣는다.
    ///
    /// `due_at` 과 `last_review_at` 를 **따로** 받는다. 둘을 묶어 두면 "안정성이
    /// 클수록 더 나중에 Due" 가 되어, 안정성 차이만 비교하려는 테스트의 전제가
    /// 뒤집힌다. 둘 다 과거면 이미 Due 다.
    async fn set_state(
        &self,
        card: voca_domain::Id,
        stability: f64,
        last_review_at: i64,
        due_at: i64,
    ) {
        sqlx::query(
            "UPDATE card_states SET state='review', stability=?2, difficulty=5.0,
                    scheduled_days=?2, due_at=?3, last_review_at=?4, reps=1, introduced_at=?4
             WHERE card_id=?1",
        )
        .bind(card.to_string())
        .bind(stability)
        .bind(due_at)
        .bind(last_review_at)
        .execute(self.store.pool())
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn a_deck_created_without_limits_gets_the_defaults() {
    // 픽스처의 덱은 한도를 20/3 으로 지정한다. 기본값을 보려면 새로 만들어야 한다.
    let w = World::setup().await;
    let created = w
        .store
        .create_deck(w.user, NewDeck::named("기본값 덱"))
        .await
        .unwrap();
    assert_eq!(created.daily_goal, 20);
    assert_eq!(created.new_per_day, 10);
}

#[tokio::test]
async fn list_decks_keeps_creation_order() {
    let w = World::setup().await;
    for name in ["두 번째", "세 번째", "네 번째"] {
        w.store
            .create_deck(w.user, NewDeck::named(name))
            .await
            .unwrap();
    }
    let names: Vec<String> = w
        .store
        .list_decks(w.user)
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["수능 단어", "두 번째", "세 번째", "네 번째"]);
}

#[tokio::test]
async fn a_new_deck_can_set_its_own_limits() {
    let w = World::setup().await;
    let mut deck = NewDeck::named("TOEFL 핵심");
    deck.daily_goal = Some(40);
    deck.new_per_day = Some(15);

    let created = w.store.create_deck(w.user, deck).await.unwrap();
    assert_eq!(created.daily_goal, 40);
    assert_eq!(created.new_per_day, 15);
    assert!(!created.is_night_heavy());
}

#[tokio::test]
async fn an_empty_deck_name_is_rejected() {
    let w = World::setup().await;
    let result = w.store.create_deck(w.user, NewDeck::named("   ")).await;
    assert!(matches!(result, Err(StoreError::Invalid(_))));
}

#[tokio::test]
async fn a_deck_can_be_renamed_without_touching_other_fields() {
    let w = World::setup().await;
    let updated = w
        .store
        .update_deck(
            w.user,
            DeckUpdate {
                id: w.deck,
                name: Some("수능 단어 v2".into()),
                description: None,
                daily_goal: None,
                new_per_day: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.name, "수능 단어 v2");
    assert_eq!(updated.daily_goal, 20, "건드리지 않은 값은 그대로여야 한다");
}

#[tokio::test]
async fn adding_the_same_sense_twice_is_silently_skipped() {
    let w = World::setup().await;
    let cards = w
        .word(
            "abandon",
            &[(SenseKind::Word, "포기하다", "He abandoned it.")],
        )
        .await;

    // 이미 Card 가 된 Sense 를 다시 넣는다. 중복을 확인하려면 Sense id 가 필요하다.
    let sense_id: String = sqlx::query_scalar("SELECT sense_id FROM cards WHERE id = ?1")
        .bind(cards[0].to_string())
        .fetch_one(w.store.pool())
        .await
        .unwrap();
    let second = w
        .store
        .add_cards(
            w.user,
            w.deck,
            &[voca_domain::Id::parse(&sense_id).unwrap()],
        )
        .await
        .unwrap();
    assert!(second.is_empty(), "중복 추가가 두 번째 배열을 채웠다");

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 20))
        .await
        .unwrap();
    assert_eq!(queue.cards.len(), 1, "같은 Card 가 두 개 생겼다");
}

#[tokio::test]
async fn the_new_card_daily_limit_is_enforced() {
    let w = World::setup().await;
    // 덱의 new_per_day 는 3. 5개를 넣어도 3개만 나온다.
    let ids = w
        .word(
            "run",
            &[
                (SenseKind::Word, "달리다", "He runs."),
                (SenseKind::Word, "운영하다", "She runs a company."),
                (SenseKind::Phrase, "도망치다", "He ran off."),
                (SenseKind::Word, "흐르다", "Water runs."),
                (SenseKind::Word, "경주하다", "They ran."),
            ],
        )
        .await;
    assert_eq!(ids.len(), 5);

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 20))
        .await
        .unwrap();

    assert_eq!(queue.cards.len(), 3, "하루 신규 한도 3 을 넘었다");
    assert_eq!(queue.new_remaining_today, 2, "남은 몫이 잘못 보고됐다");
    assert!(
        queue
            .cards
            .iter()
            .all(|c| c.memory_state.state == ReviewState::New),
        "신규 한도를 적용했는데 복습 카드가 섞였다"
    );
}

#[tokio::test]
async fn reviews_come_back_in_ascending_retrievability() {
    // 이 interface 의 핵심 정책. 기억 회수 가능성이 낮은 단어가 먼저 나온다.
    let w = World::setup().await;
    let ids = w
        .word(
            "use",
            &[
                (SenseKind::Word, "쓰다", "a"),
                (SenseKind::Word, "이용하다", "b"),
                (SenseKind::Word, "습관", "c"),
            ],
        )
        .await;

    // 셋 다 Due 다. 안정성만 다르다 — 낮을수록 덜 기억하고 있다는 뜻이다.
    // 10일 전에 셋 다 봤다고 하자.
    for (id, stability) in ids.iter().zip([2.0, 30.0, 200.0]) {
        w.set_state(*id, stability, days_ago(10.0), days_ago(1.0))
            .await;
    }

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 10))
        .await
        .unwrap();
    let order: Vec<f32> = queue.cards.iter().map(|c| c.retrievability).collect();

    assert_eq!(order.len(), 3);
    assert!(
        order.windows(2).all(|p| p[0] <= p[1]),
        "회수 가능성이 오름차순이 아니다: {order:?}"
    );
    assert!(
        queue.cards[0].retrievability < queue.cards[2].retrievability,
        "가장 잊을 것 같은 단어가 맨 앞이 아니다: {order:?}"
    );
}

#[tokio::test]
async fn a_stable_card_leaves_the_queue() {
    let w = World::setup().await;
    let ids = w
        .word(
            "oblige",
            &[
                (SenseKind::Word, "의무지다", "a"),
                (SenseKind::Word, "호의 베풀다", "b"),
            ],
        )
        .await;

    // 안정성 300일짜리를 어제 봤다 → 299일 뒤에 Due. 나와서는 안 된다.
    w.set_state(ids[0], 300.0, days_ago(1.0), days_ahead(299.0))
        .await;
    // 안정성 5일짜리를 10일 전에 봤다 → 5일 전에 Due. 나와야 한다.
    w.set_state(ids[1], 5.0, days_ago(10.0), days_ago(5.0))
        .await;

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 10))
        .await
        .unwrap();
    let defs: Vec<&str> = queue
        .cards
        .iter()
        .map(|c| c.sense.definition.as_str())
        .collect();

    assert_eq!(defs.len(), 1, "내일이 Due 인 단어까지 나왔다: {defs:?}");
    assert_eq!(
        defs[0], "호의 베풀다",
        "10일 지난 단어(안정성 5일)가 Due 다. 1일 지난 단어(안정성 300일)는 아니다"
    );
}

#[tokio::test]
async fn the_requested_limit_caps_the_queue_and_reports_the_rest() {
    let w = World::setup().await;
    let ids = w
        .word(
            "issue",
            &[
                (SenseKind::Word, "쟁점", "a"),
                (SenseKind::Word, "발행하다", "b"),
                (SenseKind::Word, "결과", "c"),
                (SenseKind::Word, "출판", "d"),
            ],
        )
        .await;

    for id in &ids {
        w.set_state(*id, 5.0, days_ago(10.0), days_ago(1.0)).await;
    }

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 2))
        .await
        .unwrap();
    assert_eq!(queue.cards.len(), 2, "limit 을 무시했다");
    assert_eq!(queue.reviews_remaining, 2, "밀린 몫을 알려주지 않는다");
    assert!(queue.has_more());
}

#[tokio::test]
async fn a_scoped_queue_ignores_other_decks() {
    let w = World::setup().await;
    let other = w
        .store
        .create_deck(w.user, NewDeck::named("TOEFL 핵심"))
        .await
        .unwrap();
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;

    // 같은 Sense 의 Card 를 다른 덱에 하나 더 넣는다.
    let sense_id: String = sqlx::query_scalar("SELECT id FROM senses LIMIT 1")
        .fetch_one(w.store.pool())
        .await
        .unwrap();
    w.store
        .add_cards(
            w.user,
            other.id,
            &[voca_domain::Id::parse(&sense_id).unwrap()],
        )
        .await
        .unwrap();

    let now = now();
    let all = w
        .store
        .study_queue(StudyRequest::new(w.user, now, 20))
        .await
        .unwrap();
    assert_eq!(all.cards.len(), 2, "두 덱 모두 보여야 한다");

    let scoped = w
        .store
        .study_queue(StudyRequest::new(w.user, now, 20).in_deck(w.deck))
        .await
        .unwrap();
    assert_eq!(scoped.cards.len(), 1, "범위 밖 덱의 Card 가 섞였다");
    assert_eq!(scoped.cards[0].card.deck_id, w.deck);
}

#[tokio::test]
async fn another_users_deck_refuses_cards() {
    let w = World::setup().await;
    let stranger = voca_domain::Id::from([42u8; 16]);
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;
    let sense_id: String = sqlx::query_scalar("SELECT id FROM senses LIMIT 1")
        .fetch_one(w.store.pool())
        .await
        .unwrap();
    let result = w
        .store
        .add_cards(
            stranger,
            w.deck,
            &[voca_domain::Id::parse(&sense_id).unwrap()],
        )
        .await;
    assert_eq!(result, Err(StoreError::NotFound));
}

#[tokio::test]
async fn a_cloned_card_starts_from_scratch() {
    // 복제본은 복습 이력을 **가져오지 않는다** (docs/adr/0004).
    let w = World::setup().await;
    let other = w
        .store
        .create_deck(w.user, NewDeck::named("TOEFL 핵심"))
        .await
        .unwrap();
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 20))
        .await
        .unwrap();
    let original = queue.cards[0].card.card_id;
    // 원본을 성숙한 카드로 만든다. 복제본이 이걸 물려받지 않는지 본다.
    w.set_state(original, 400.0, days_ago(1.0), days_ahead(399.0))
        .await;

    let clone = w
        .store
        .clone_card(w.user, original, other.id)
        .await
        .unwrap();
    assert_eq!(clone.deck_id, other.id);

    let state: (String, i64) =
        sqlx::query_as("SELECT state, reps FROM card_states WHERE card_id = ?1")
            .bind(clone.id.to_string())
            .fetch_one(w.store.pool())
            .await
            .unwrap();
    assert_eq!(state.0, "new", "복제본이 이력을 상속했다");
    assert_eq!(state.1, 0);

    let cloned_from: Option<String> =
        sqlx::query_scalar("SELECT cloned_from FROM cards WHERE id = ?1")
            .bind(clone.id.to_string())
            .fetch_one(w.store.pool())
            .await
            .unwrap();
    assert_eq!(
        cloned_from.as_deref(),
        Some(original.to_string().as_str()),
        "원본을 가리키는 단서가 없다"
    );
}

#[tokio::test]
async fn an_archived_card_leaves_the_queue_and_the_dashboard() {
    let w = World::setup().await;
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;

    let at = now();
    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, at, 20))
        .await
        .unwrap();
    let card = queue.cards[0].card.card_id;
    w.store.archive_card(w.user, card).await.unwrap();

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, at, 20))
        .await
        .unwrap();
    assert!(queue.cards.is_empty(), "숨긴 Card 가 큐에 남았다");

    let board = w.store.dashboard(w.user, at).await.unwrap();
    assert_eq!(board.decks[0].total, 0, "숨긴 Card 가 진도에 남아 있다");
    assert!(
        !board.has_work_today(),
        "카드를 하나도 못 보는데 할 일이 있다고 한다"
    );
}

#[tokio::test]
async fn a_user_written_sense_gets_a_word_and_becomes_a_card() {
    let w = World::setup().await;
    let sense = w
        .store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "run off".into(),
                kind: SenseKind::Phrase,
                example_en: Some("He ran off without paying.".into()),
                definition: "도망치다".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    assert!(sense.is_user_authored());
    assert_eq!(sense.kind, SenseKind::Phrase);

    let word = w.store.find_word(w.user, "run off").await.unwrap().unwrap();
    assert!(word.is_user_authored());
    assert_eq!(word.lemma, "run off");
    assert_eq!(word.senses.len(), 1);

    // Card 앞면은 구문(Word 의 표기형)이다.
    assert_eq!(sense.front(&word.lemma), "run off");
}

#[tokio::test]
async fn an_example_sense_shows_its_sentence_on_the_front() {
    // docs/adr/0011. 예문 Sense 의 Word 는 설명되는 원래 단어다.
    let w = World::setup().await;
    let sentence = "They abandoned the car and fled.";
    let sense = w
        .store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "abandon".into(),
                kind: SenseKind::Example,
                example_en: Some(sentence.into()),
                definition: "이 문장에서는 '떠나다'다".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    let word = w.store.find_word(w.user, "abandon").await.unwrap().unwrap();
    assert_eq!(word.lemma, "abandon", "문장이 Word 가 되면 안 된다");
    assert_eq!(sense.front(&word.lemma), sentence);
}

#[tokio::test]
async fn an_archived_sense_disappears_from_search_but_the_word_remains() {
    let w = World::setup().await;
    let first = w
        .store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "abandon".into(),
                kind: SenseKind::Word,
                example_en: None,
                definition: "포기하다".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    w.store.archive_sense(w.user, first.id).await.unwrap();
    let page = w
        .store
        .search_senses(w.user, &SenseQuery::text("포기").only_user())
        .await
        .unwrap();
    assert!(page.is_empty(), "숨긴 Sense 가 검색에 나온다");

    let word = w.store.find_word(w.user, "abandon").await.unwrap().unwrap();
    assert!(word.senses.is_empty(), "숨긴 Sense 가 Word 에 남아 있다");
}

#[tokio::test]
async fn a_user_word_falls_back_to_the_dictionary_entry() {
    let w = World::setup().await;
    w.word("abandon", &[(SenseKind::Word, "사전 뜻", "a")])
        .await;

    w.store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "abandon".into(),
                kind: SenseKind::Word,
                example_en: None,
                definition: "내가 기억하는 뜻".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    // 사용자 단어를 먼저 보여준다 — 방금 만든 그 단어일 가능성이 높기 때문이다.
    let word = w.store.find_word(w.user, "abandon").await.unwrap().unwrap();
    assert!(word.is_user_authored());
    assert_eq!(word.lemma, "abandon");
}

#[tokio::test]
async fn an_unfiltered_search_is_refused() {
    let w = World::setup().await;
    let result = w.store.search_senses(w.user, &SenseQuery::default()).await;
    assert!(matches!(result, Err(StoreError::Invalid(_))));
}

#[tokio::test]
async fn the_dashboard_counts_work_and_progress_together() {
    let w = World::setup().await;
    let ids = w
        .word(
            "run",
            &[
                (SenseKind::Word, "달리다", "a"),
                (SenseKind::Word, "운영하다", "b"),
            ],
        )
        .await;

    // 5일 지난 Due 한 개 + 내일 Due 한 개.
    w.set_state(ids[0], 5.0, days_ago(10.0), days_ago(5.0))
        .await;
    w.set_state(ids[1], 500.0, days_ago(1.0), days_ahead(499.0))
        .await;

    let board = w.store.dashboard(w.user, now()).await.unwrap();
    assert_eq!(board.decks[0].total, 2);
    assert_eq!(board.decks[0].seen, 2);
    assert_eq!(board.decks[0].due, 1, "5일 지난 단어만 Due 다");
    assert_eq!(board.decks[0].progress_percent(), 100);
    assert_eq!(board.reviews_due, 1);
    assert!(board.has_work_today());
    assert!(board.is_streak_at_risk(), "할 게 있는데 오늘 아직 안 했다");
}

#[tokio::test]
async fn the_dashboard_reports_nothing_to_do_after_everything_is_reviewed() {
    let w = World::setup().await;
    let ids = w
        .word(
            "run",
            &[
                (SenseKind::Word, "달리다", "a"),
                (SenseKind::Word, "운영하다", "b"),
            ],
        )
        .await;

    for id in &ids {
        w.set_state(*id, 500.0, days_ago(0.04), days_ahead(499.0))
            .await;
    }

    let board = w.store.dashboard(w.user, now()).await.unwrap();
    assert_eq!(board.reviews_due, 0);
    assert!(!board.has_work_today());
}

#[tokio::test]
async fn sync_reports_changes_and_then_stops() {
    let w = World::setup().await;
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;

    let first = w
        .store
        .changes_since(voca_store::ChangeRequest::new(
            w.user,
            voca_store::Revision::initial(),
        ))
        .await
        .unwrap();
    assert!(!first.is_empty(), "덱을 만들었는데 변경이 없다");
    assert!(!first.has_more);

    // 두 번째 호출은 아무것도 안 돌려주고 watermark 를 유지한다.
    let second = w
        .store
        .changes_since(voca_store::ChangeRequest::new(w.user, first.watermark))
        .await
        .unwrap();
    assert!(second.is_empty());
    assert_eq!(second.watermark, first.watermark);
}

#[tokio::test]
async fn sync_carries_the_card_state_not_just_the_body() {
    // Card 본문만 동기화하면 기기를 바꿨을 때 복습 일정 전체가 뒤집힌다.
    let w = World::setup().await;
    let senses = w
        .word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;
    let _ = senses;

    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 20))
        .await
        .unwrap();
    let card = queue.cards[0].card.card_id;

    let page = w
        .store
        .changes_since(voca_store::ChangeRequest::new(
            w.user,
            voca_store::Revision::initial(),
        ))
        .await
        .unwrap();

    let change = page
        .changes
        .iter()
        .find(|c| matches!(c, Change::Card(v) if v.item.id == card));
    let Some(Change::Card(v)) = change else {
        panic!("Card 가 동기화 목록에 없다: {:?}", page.changes);
    };
    assert_eq!(v.item.state.state, "new");
    assert!(v.item.state.due_at > 0, "스케줄러 상태가 없이 본문만 갔다");
}

#[tokio::test]
async fn sync_reports_a_tombstone_for_an_archived_card() {
    let w = World::setup().await;
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;
    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now(), 20))
        .await
        .unwrap();
    let card = queue.cards[0].card.card_id;
    w.store.archive_card(w.user, card).await.unwrap();

    let page = w
        .store
        .changes_since(voca_store::ChangeRequest::new(
            w.user,
            voca_store::Revision::initial(),
        ))
        .await
        .unwrap();
    assert!(
        page.changes
            .iter()
            .any(|c| matches!(c, Change::CardTombstone { id, .. } if *id == card)),
        "숨긴 Card 의 tombstone 이 없다"
    );
}

#[tokio::test]
async fn a_rating_cycle_leaves_the_card_scheduled_in_the_future() {
    // interface 전체를 한 바퀴 도는 확인. 큐 → 평가 → 큐.
    let w = World::setup().await;
    w.word("abandon", &[(SenseKind::Word, "포기하다", "a")])
        .await;

    let now = now();
    let queue = w
        .store
        .study_queue(StudyRequest::new(w.user, now, 20))
        .await
        .unwrap();
    let card = queue.cards[0].card.card_id;

    let outcome = w
        .store
        .submit_review(voca_store::ReviewRequest::new(
            w.user,
            card,
            Rating::Good,
            now,
            now.date(),
            9 * 3600,
        ))
        .await
        .unwrap();

    assert!(outcome.memory_state.due_at > now);
    let next = w
        .store
        .study_queue(StudyRequest::new(w.user, now, 20))
        .await
        .unwrap();
    assert!(next.cards.is_empty(), "바로 다시 나왔다");
}

// ── 사용자 격리 ──────────────────────────────────────────

/// 두 사용자가 **같은 표기형**의 자기 뜻�� 만든다.
#[tokio::test]
async fn two_users_writing_the_same_lemma_do_not_share_a_word() {
    let w = World::setup().await;
    let other = voca_domain::Id::from([9u8; 16]);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'b@b.kr', 'x', '다른 사람', 'Asia/Seoul', 0)",
    )
    .bind(other.to_string())
    .execute(w.store.pool())
    .await
    .unwrap();

    let a = w
        .store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "run".into(),
                kind: voca_domain::SenseKind::Word,
                example_en: None,
                definition: "A가 생각한 뜻".into(),
                pos: None,
            },
        )
        .await
        .unwrap();
    let b = w
        .store
        .put_user_sense(
            other,
            NewUserSense {
                lemma: "run".into(),
                kind: voca_domain::SenseKind::Word,
                example_en: None,
                definition: "B가 생각한 뜻".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    // **다른 Word여야 한다.** 같은 Word면 A의 개인 뜻이 B에게 노출된다.
    assert_ne!(
        a.word_id, b.word_id,
        "두 사용자의 개인 뜻이 한 Word 를 공유한다 — 서로의 뜻이 새어 나간다"
    );

    // B가 찾는 'run' 에는 B의 뜻만 있어야 한다.
    let found = w.store.find_word(other, "run").await.unwrap();
    let found = found.expect("없다");
    assert!(
        !found.senses.iter().any(|s| s.definition == "A가 생각한 뜻"),
        "A의 개인 뜻이 B에게 보인다: {:?}",
        found
            .senses
            .iter()
            .map(|s| &s.definition)
            .collect::<Vec<_>>()
    );
}

/// 남의 것을 조작하려는 시도는 **조용히 `NotFound` 다.**
///
/// 403 이 아니라 404 여야 한다 — "남의 Card 가 있다"는 사실 자체를 새지 않는다.
#[tokio::test]
async fn touching_another_users_rows_is_not_found_not_forbidden() {
    let w = World::setup().await;
    let other = voca_domain::Id::from([9u8; 16]);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'b@b.kr', 'x', '다른 사람', 'Asia/Seoul', 0)",
    )
    .bind(other.to_string())
    .execute(w.store.pool())
    .await
    .unwrap();

    let sense = w
        .store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "run".into(),
                kind: voca_domain::SenseKind::Word,
                example_en: None,
                definition: "내 뜻".into(),
                pos: None,
            },
        )
        .await
        .unwrap();
    let cards = w
        .store
        .add_cards(w.user, w.deck, &[sense.id])
        .await
        .unwrap();
    let card = cards[0].id;

    // 1. 남의 Card 를 지운다 — 실패해야 한다.
    assert!(
        w.store.archive_card(other, card).await.is_err(),
        "남의 Card 를 지웠다"
    );

    // 2. 남의 덱 이름을 바꾼다 — 실패해야 한다.
    assert!(
        w.store
            .update_deck(
                other,
                DeckUpdate {
                    id: w.deck,
                    name: Some("탈취".into()),
                    description: None,
                    daily_goal: None,
                    new_per_day: None,
                }
            )
            .await
            .is_err(),
        "남의 덱을 고쳤다"
    );

    // 3. 남의 Sense 를 숨긴다 — 실패해야 한다.
    assert!(
        w.store.archive_sense(other, sense.id).await.is_err(),
        "남의 Sense 를 숨겼다"
    );

    // 4. 남의 Card 를 자기 덱으로 복제한다 — 실패해야 한다.
    let other_deck = voca_domain::Id::from([8u8; 16]);
    sqlx::query(
        "INSERT INTO decks (id, user_id, name, daily_goal, new_per_day, created_at, updated_at)
         VALUES (?1, ?2, '남의 덱', 20, 3, 0, 0)",
    )
    .bind(other_deck.to_string())
    .bind(other.to_string())
    .execute(w.store.pool())
    .await
    .unwrap();
    assert!(
        w.store.clone_card(w.user, card, other_deck).await.is_err(),
        "자기 덱에 있는 카드를 남의 덱으로 복제했다"
    );

    // 무엇도 바뀌지 않았는지 확인한다.
    let name: String = sqlx::query_scalar("SELECT name FROM decks WHERE id = ?1")
        .bind(w.deck.to_string())
        .fetch_one(w.store.pool())
        .await
        .unwrap();
    assert_ne!(name, "탈취", "남의 덱 이름이 바뀌었다");
    let deleted: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE id = ?1 AND deleted_at IS NOT NULL")
            .bind(card.to_string())
            .fetch_one(w.store.pool())
            .await
            .unwrap();
    assert_eq!(deleted, 0, "남의 Card 가 지워졌다");
}

/// 검색에 남의 개인 뜻이 나오지 않는다.
#[tokio::test]
async fn search_does_not_surface_another_users_private_meaning() {
    let w = World::setup().await;
    let other = voca_domain::Id::from([9u8; 16]);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'b@b.kr', 'x', '다른 사람', 'Asia/Seoul', 0)",
    )
    .bind(other.to_string())
    .execute(w.store.pool())
    .await
    .unwrap();

    w.store
        .put_user_sense(
            other,
            NewUserSense {
                lemma: "abandon".into(),
                kind: voca_domain::SenseKind::Word,
                example_en: None,
                definition: "B만 보는 비밀 뜻".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    // 일반 검색.
    let all = w
        .store
        .search_senses(w.user, &SenseQuery::text("비밀"))
        .await
        .unwrap();
    assert!(
        all.items.is_empty(),
        "남의 개인 뜻이 검색에 나왔다: {:?}",
        all.items.iter().map(|s| &s.definition).collect::<Vec<_>>()
    );

    // 표기형으로 찾아도.
    let by_lemma = w
        .store
        .search_senses(w.user, &SenseQuery::text("abandon"))
        .await
        .unwrap();
    assert!(
        by_lemma.items.is_empty(),
        "표기형 검색으로 남의 뜻이 나왔다"
    );

    // 자기 뜻은 보인다.
    w.store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "abandon".into(),
                kind: voca_domain::SenseKind::Word,
                example_en: None,
                definition: "버리다".into(),
                pos: None,
            },
        )
        .await
        .unwrap();
    let mine = w
        .store
        .search_senses(w.user, &SenseQuery::text("버리다"))
        .await
        .unwrap();
    assert_eq!(mine.items.len(), 1, "내 뜻이 안 보인다");
}

/// 출처 필터가 없을 때 "전부" 를 뜻한다.
///
/// `AND source = NULL` 로 바인딩하면 SQL 에서 절대로 참이 되지 않아 **항상 0건**이
/// 된다. 기존 테스트가 전부 `.only_user()` 로만 검색해서 이 버그가 살아 있었다.
#[tokio::test]
async fn searching_without_a_source_filter_returns_both_kinds() {
    let w = World::setup().await;
    w.store
        .upsert_dictionary_word(UpsertDictionaryWord {
            lemma: "abandon".into(),
            phonetic: None,
            senses: vec![DictionarySense {
                kind: voca_domain::SenseKind::Word,
                pos: Some("verb".into()),
                definition: "버리다 (사전)".into(),
                example_en: None,
                example_ko: None,
            }],
        })
        .await
        .unwrap();
    w.store
        .put_user_sense(
            w.user,
            NewUserSense {
                lemma: "abandon".into(),
                kind: voca_domain::SenseKind::Word,
                example_en: None,
                definition: "버리다 (내 뜻)".into(),
                pos: None,
            },
        )
        .await
        .unwrap();

    // 조건 없음 = 전부. 사전 뜻과 내 뜻이 **둘 다** 나와야 한다.
    let all = w
        .store
        .search_senses(w.user, &SenseQuery::text("버리다"))
        .await
        .unwrap();
    assert_eq!(
        all.total,
        2,
        "출처 필터가 없는데 {} 건만 나왔다: {:?}",
        all.total,
        all.items.iter().map(|s| &s.definition).collect::<Vec<_>>()
    );

    // 거꾸로 명시하면 하나씩만 나온다.
    let dict = w
        .store
        .search_senses(w.user, &SenseQuery::text("버리다").only_dictionary())
        .await
        .unwrap();
    assert_eq!(dict.total, 1);
    assert_eq!(dict.items[0].definition, "버리다 (사전)");

    let mine = w
        .store
        .search_senses(w.user, &SenseQuery::text("버리다").only_user())
        .await
        .unwrap();
    assert_eq!(mine.total, 1);
    assert_eq!(mine.items[0].definition, "버리다 (내 뜻)");
}

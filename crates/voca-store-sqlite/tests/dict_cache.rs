//! 사전 캐시와 전역 단어 저장.
//!
//! 캐시의 목적을 그대로 시험한다: **같은 단어를 두 번째로 추가할 때 API 를 다시
//! 부르지 않는다.**

use tempfile::TempDir;
use voca_store::Store;
use voca_store::{DictionarySense, SenseKind, UpsertDictionaryWord};
use voca_store_sqlite::{FetchStatus, SqliteStore};

/// 테스트 계정. 사용자 Word 는 `user_id` 로 갈라지므로 항상 필요하다.
fn test_user() -> voca_domain::Id {
    voca_domain::Id::from([7u8; 16])
}

async fn store() -> (TempDir, SqliteStore) {
    let dir = TempDir::new().unwrap();
    let s = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 't@b.kr', 'x', '테스터', 'Asia/Seoul', 0)",
    )
    .bind(test_user().to_string())
    .execute(s.pool())
    .await
    .unwrap();
    (dir, s)
}

fn word(lemma: &str, definitions: &[&str]) -> UpsertDictionaryWord {
    UpsertDictionaryWord {
        lemma: lemma.into(),
        phonetic: Some("/rʌn/".into()),
        senses: definitions
            .iter()
            .map(|d| DictionarySense {
                kind: SenseKind::Word,
                pos: Some("verb".into()),
                definition: (*d).into(),
                example_en: Some("an example".into()),
                example_ko: None,
            })
            .collect(),
    }
}

#[tokio::test]
async fn an_empty_cache_returns_none() {
    let (_d, s) = store().await;
    assert!(s.dict_cache_get("run").await.unwrap().is_none());
}

#[tokio::test]
async fn a_cached_response_comes_back() {
    let (_d, s) = store().await;
    s.dict_cache_put("run", "{\"word\":\"run\"}", FetchStatus::Ok, 600)
        .await
        .unwrap();

    let (body, status) = s.dict_cache_get("run").await.unwrap().unwrap();
    assert!(body.contains("run"));
    assert_eq!(status, FetchStatus::Ok);
}

#[tokio::test]
async fn a_successful_entry_never_expires() {
    // 사전의 뜻은 자주 안 바뀐다. 성공한 항목을 시간 지나게 쫓아내면 API 를
    // 계속 부르게 되고, 그게 캐시 없는 것보다 나쁘다.
    let (_d, s) = store().await;
    s.dict_cache_put("run", "{}", FetchStatus::Ok, 1)
        .await
        .unwrap();

    // `SELECT expires_at` 로 NULL 을 Option 으로 받는 것은 디코더에 기대게 된다.
    // `IS NULL` 로 직접 물어본다.
    let is_null: i64 =
        sqlx::query_scalar("SELECT expires_at IS NULL FROM dict_cache WHERE lemma = 'run'")
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(is_null, 1, "성공한 항목에 만료 시각이 있다");
    assert!(s.dict_cache_get("run").await.unwrap().is_some());
}

#[tokio::test]
async fn a_failed_entry_expires_and_is_then_refetched() {
    let (_d, s) = store().await;
    s.dict_cache_put("run", "", FetchStatus::Error, 60)
        .await
        .unwrap();

    // 시간제한을 흉내 내기 위해 만료를 뒤로 옮긴다.
    sqlx::query("UPDATE dict_cache SET expires_at = 1 WHERE lemma = 'run'")
        .execute(s.pool())
        .await
        .unwrap();

    assert!(
        s.dict_cache_get("run").await.unwrap().is_none(),
        "만료된 실패 항목이 계속 쓰인다"
    );
}

#[tokio::test]
async fn the_cache_is_case_insensitive() {
    // 사용자는 "Run" 과 "run" 을 같은 단어로 본다.
    let (_d, s) = store().await;
    s.dict_cache_put("Run", "{\"word\":\"Run\"}", FetchStatus::Ok, 600)
        .await
        .unwrap();
    assert!(s.dict_cache_get("run").await.unwrap().is_some());
    assert!(s.dict_cache_get("RUN").await.unwrap().is_some());
}

#[tokio::test]
async fn writing_twice_replaces_rather_than_duplicating() {
    let (_d, s) = store().await;
    s.dict_cache_put("run", "{\"v\":1}", FetchStatus::Ok, 600)
        .await
        .unwrap();
    s.dict_cache_put("run", "{\"v\":2}", FetchStatus::Ok, 600)
        .await
        .unwrap();

    let (body, _) = s.dict_cache_get("run").await.unwrap().unwrap();
    assert!(body.contains("\"v\":2"));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dict_cache")
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn a_cached_absence_stays_cached() {
    // 없는 단어를 매번 물어보면 API 를 소진한다.
    let (_d, s) = store().await;
    s.dict_cache_put("asdfghjkl", "[]", FetchStatus::NotFound, 600)
        .await
        .unwrap();

    let (body, status) = s.dict_cache_get("asdfghjkl").await.unwrap().unwrap();
    assert_eq!(body, "[]");
    assert_eq!(status, FetchStatus::NotFound);
}

#[tokio::test]
async fn forgetting_drops_the_entry() {
    let (_d, s) = store().await;
    s.dict_cache_put("run", "{}", FetchStatus::Ok, 600)
        .await
        .unwrap();
    s.dict_cache_forget("run").await.unwrap();
    assert!(s.dict_cache_get("run").await.unwrap().is_none());
}

// ── 전역 단어 저장 ───────────────────────────────────────

#[tokio::test]
async fn a_dictionary_word_is_stored_with_every_meaning() {
    let (_d, s) = store().await;
    let view = s
        .upsert_dictionary_word(word("run", &["to move fast", "to manage"]))
        .await
        .unwrap();

    assert_eq!(view.lemma, "run");
    assert_eq!(view.senses.len(), 2, "다의어가 하나로 합쳐졌다");
    assert!(!view.is_user_authored());
    assert_eq!(view.phonetic.as_deref(), Some("/rʌn/"));
}

#[tokio::test]
async fn storing_the_same_word_twice_does_not_duplicate_senses() {
    // 같은 API 응답이 두 번 도착할 수 있다. 그때 뜻이 두 배로 쌓이면 안 된다.
    let (_d, s) = store().await;
    s.upsert_dictionary_word(word("run", &["to move fast"]))
        .await
        .unwrap();
    let view = s
        .upsert_dictionary_word(word("run", &["to move fast", "to manage"]))
        .await
        .unwrap();

    assert_eq!(view.senses.len(), 2);
}

#[tokio::test]
async fn a_dictionary_word_is_visible_to_every_user() {
    // 사전은 공유된다. `source` 로 구분된다 (docs/adr/0007).
    let (_d, s) = store().await;
    s.upsert_dictionary_word(word("run", &["to move fast"]))
        .await
        .unwrap();

    let found = s.find_word(test_user(), "run").await.unwrap();
    let found = found.expect("사전 단어가 없다");
    assert!(!found.is_user_authored());
    assert_eq!(found.senses.len(), 1);
}

#[tokio::test]
async fn a_user_word_and_a_dictionary_word_coexist() {
    // 사용자가 같은 표기형에 자기 뜻을 지어도 사전 뜻이 사라지지 않는다.
    let (_d, s) = store().await;
    s.upsert_dictionary_word(word("run", &["사전 뜻"]))
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO words (id, lemma, source, user_id, updated_at)
         VALUES ('11111111-1111-7111-8111-111111111111', 'run', 'user', ?1, 0)",
    )
    .bind(test_user().to_string())
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO senses (id, word_id, kind, source, definition, updated_at)
         VALUES ('22222222-2222-7222-8222-222222222222',
                 '11111111-1111-7111-8111-111111111111',
                 'word', 'user', '내 뜻', 0)",
    )
    .execute(s.pool())
    .await
    .unwrap();

    // 사용자 단어가 먼저 나온다 — 방금 만든 그 단어일 가능성이 높기 때문이다.
    let found = s.find_word(test_user(), "run").await.unwrap().unwrap();
    assert!(found.is_user_authored());
    assert_eq!(found.senses.len(), 1);
    assert_eq!(found.senses[0].definition, "내 뜻");

    let dictionary_sense: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM senses WHERE source = 'dictionary' AND definition = '사전 뜻'",
    )
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        dictionary_sense, 1,
        "사용자 단어를 넣었는데 사전 뜻이 사라졌다"
    );
}

#[tokio::test]
async fn a_word_with_no_meanings_is_refused() {
    // 뜻 없는 단어가 들어가면 큐에서 아무것도 안 나온다. 조용히 넣지 않는다.
    let (_d, s) = store().await;
    let result = s.upsert_dictionary_word(word("run", &[])).await;
    assert!(matches!(result, Err(voca_store::StoreError::Invalid(_))));
}

#[tokio::test]
async fn a_blank_lemma_is_refused() {
    let (_d, s) = store().await;
    let result = s.upsert_dictionary_word(word("   ", &["뜻"])).await;
    assert!(matches!(result, Err(voca_store::StoreError::Invalid(_))));
}

#[tokio::test]
async fn a_blank_definition_is_refused_and_rolls_back() {
    // 하나라도 비면 전체를 거부한다. 중간까지만 들어간 상태는 안 남는다.
    let (_d, s) = store().await;
    let mut w = word("run", &["좋은 뜻"]);
    w.senses.push(DictionarySense {
        kind: SenseKind::Word,
        pos: None,
        definition: "   ".into(),
        example_en: None,
        example_ko: None,
    });

    assert!(s.upsert_dictionary_word(w).await.is_err());

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM senses")
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(count, 0, "거절했는데 앞의 뜻은 들어갔다");
}

#[tokio::test]
async fn a_positional_card_can_be_added_to_a_dictionary_word() {
    // 사전에서 받아 온 다의어가 곧바로 Card 로 쓰여야 한다.
    let (_d, s) = store().await;
    let view = s
        .upsert_dictionary_word(word("run", &["to move fast", "to manage"]))
        .await
        .unwrap();

    let user = voca_store::UserId::from([1u8; 16]);
    let deck = voca_store::DeckId::from([2u8; 16]);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'a@b.kr', 'x', 't', 'UTC', 0)",
    )
    .bind(user.to_string())
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO decks (id, user_id, name, created_at, updated_at)
         VALUES (?1, ?2, '수능 단어', 0, 0)",
    )
    .bind(deck.to_string())
    .bind(user.to_string())
    .execute(s.pool())
    .await
    .unwrap();

    let sense_ids: Vec<voca_store::SenseId> = view.senses.iter().map(|s| s.id).collect();
    let cards = s.add_cards(user, deck, &sense_ids).await.unwrap();
    assert_eq!(cards.len(), 2, "다의어에서 두 장이 나와야 한다");
}

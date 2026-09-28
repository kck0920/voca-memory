//! 외부 사전 캐시.
//!
//! **API 를 두 번 부르지 않게 하는 것이 목적이다.** 호출마다 지연이 생기고 공개
//! API 에는 호출 한도가 있다. 같은 단어를 두 번째로 추가할 때 또 기다려야 하는
//! 경험이 없어야 한다.

use crate::SqliteStore;
use voca_store::{StoreError, StoreResult, UpsertDictionaryWord};

/// 외부 응답의 상태 코드.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchStatus {
    Ok,
    NotFound,
    Error,
}

impl FetchStatus {
    fn to_code(self) -> i64 {
        match self {
            FetchStatus::Ok => 200,
            FetchStatus::NotFound => 404,
            FetchStatus::Error => 599,
        }
    }

    fn from_code(code: i64) -> Self {
        match code {
            200 => FetchStatus::Ok,
            404 => FetchStatus::NotFound,
            _ => FetchStatus::Error,
        }
    }
}

impl SqliteStore {
    /// 캐시에서 찾는다. 유효 기간이 지나지 않았을 때만 `Some` 을 돌려준다.
    pub async fn dict_cache_get(&self, lemma: &str) -> StoreResult<Option<(String, FetchStatus)>> {
        let row: Option<(String, i64, Option<i64>)> = sqlx::query_as(
            "SELECT body, status, expires_at FROM dict_cache WHERE lemma = ?1 COLLATE NOCASE",
        )
        .bind(lemma.trim())
        .fetch_optional(self.pool())
        .await
        .map_err(crate::classify)?;

        let Some((body, status, expires_at)) = row else {
            return Ok(None);
        };

        // 유효 기간이 지났다. **지우지는 않는다** — 조용히 지우면 다음 조회가 또
        // API 를 부르고, 지웠다는 사실이 사라진다. 여기서는 `None` 을 돌려 호출자가
        // 다시 부르게 한다.
        if let Some(expiry) = expires_at
            && expiry > 0
            && expiry <= crate::now_unix_seconds()
        {
            return Ok(None);
        }

        Ok(Some((body, FetchStatus::from_code(status))))
    }

    /// 캐시에 넣는다.
    ///
    /// 성공한 항목은 만료 없이 둔다 — 사전의 뜻은 자주 안 바뀐다. 실패한 항목만
    /// 짧게 둬서 곧 재시도하게 한다.
    pub async fn dict_cache_put(
        &self,
        lemma: &str,
        body: &str,
        status: FetchStatus,
        failure_ttl_seconds: i64,
    ) -> StoreResult<()> {
        let now = crate::now_unix_seconds();
        let expires = match status {
            FetchStatus::Ok => None,
            _ => Some(now + failure_ttl_seconds.max(60)),
        };

        sqlx::query(
            "INSERT INTO dict_cache (lemma, source, body, status, fetched_at, expires_at)
             VALUES (?1, 'dictionaryapi.dev', ?2, ?3, ?4, ?5)
             ON CONFLICT(lemma) DO UPDATE SET
                 body = excluded.body,
                 status = excluded.status,
                 fetched_at = excluded.fetched_at,
                 expires_at = excluded.expires_at",
        )
        .bind(lemma.trim())
        .bind(body)
        .bind(status.to_code())
        .bind(now)
        .bind(expires)
        .execute(self.pool())
        .await
        .map_err(crate::classify)?;

        Ok(())
    }

    /// 캐시에서 비운다. 단어 데이터를 지울 때 함께 부른다.
    pub async fn dict_cache_forget(&self, lemma: &str) -> StoreResult<()> {
        sqlx::query("DELETE FROM dict_cache WHERE lemma = ?1 COLLATE NOCASE")
            .bind(lemma.trim())
            .execute(self.pool())
            .await
            .map_err(crate::classify)?;
        Ok(())
    }

    /// 사전 단어를 저장한다.
    ///
    /// **전역이라 모든 사용자에게 보인다.** 사용자 단어와 다른 출처로 구분된다
    /// (docs/adr/0007).
    pub async fn upsert_dictionary_word(
        &self,
        input: UpsertDictionaryWord,
    ) -> StoreResult<voca_store::WordView> {
        let lemma = input.lemma.trim().to_owned();
        if lemma.is_empty() {
            return Err(StoreError::Invalid("표기형이 비었다"));
        }
        if input.senses.is_empty() {
            return Err(StoreError::Invalid("뜻이 하나도 없다"));
        }

        let mut tx = self.begin().await?;
        let now = crate::now_unix_seconds();

        // 단어는 전역이라 한 명만 만들어야 한다. 둘이 동시에 만들면 UNIQUE 제약이
        // 한쪽을 거절한다 — 그쪽은 다시 읽으면 된다.
        let word_id = match sqlx::query_scalar::<_, String>(
            "SELECT id FROM words WHERE lemma = ?1 COLLATE NOCASE AND source = 'dictionary'",
        )
        .bind(&lemma)
        .fetch_optional(&mut *tx)
        .await
        .map_err(crate::classify)?
        {
            Some(existing) => existing,
            None => {
                let fresh = new_id();
                match sqlx::query(
                    "INSERT OR IGNORE INTO words (id, lemma, source, phonetic, rev, updated_at)
                     VALUES (?1, ?2, 'dictionary', ?3, 1, ?4)",
                )
                .bind(&fresh)
                .bind(&lemma)
                .bind(input.phonetic.as_deref().map(str::trim))
                .bind(now)
                .execute(&mut *tx)
                .await
                {
                    Ok(_) => fresh,
                    Err(sqlx::Error::Database(ref db)) if db.is_unique_violation() => {
                        // 다른 요청이 먼저 만들었다. 그걸 쓴다.
                        sqlx::query_scalar(
                            "SELECT id FROM words WHERE lemma = ?1 COLLATE NOCASE AND source = 'dictionary'",
                        )
                        .bind(&lemma)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(crate::classify)?
                    }
                    Err(e) => return Err(crate::classify(e)),
                }
            }
        };

        for sense in &input.senses {
            if sense.definition.trim().is_empty() {
                return Err(StoreError::Invalid("뜻이 비었다"));
            }
            // 같은 Sense 를 두 번 넣지 않는다. 부분 인덱스가 막아주지만 여기서
            // 판단한다 — 조용히 건너뛰는 쪽이 호출부에 낫다.
            let already = sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM senses WHERE word_id = ?1 AND kind = ?2 AND definition = ?3",
            )
            .bind(&word_id)
            .bind(sense_kind(sense.kind))
            .bind(sense.definition.trim())
            .fetch_optional(&mut *tx)
            .await
            .map_err(crate::classify)?;

            if already.is_some() {
                continue;
            }

            sqlx::query(
                "INSERT INTO senses
                     (id, word_id, kind, source, pos, definition, example_en, example_ko,
                      rev, updated_at)
                 VALUES (?1, ?2, ?3, 'dictionary', ?4, ?5, ?6, ?7, 1, ?8)",
            )
            .bind(new_id())
            .bind(&word_id)
            .bind(sense_kind(sense.kind))
            .bind(sense.pos.as_deref().map(str::trim))
            .bind(sense.definition.trim())
            .bind(sense.example_en.as_deref().map(str::trim))
            .bind(sense.example_ko.as_deref().map(str::trim))
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(crate::classify)?;
        }

        tx.commit().await.map_err(crate::classify)?;

        let id = crate::store_impl::parse_id(&word_id)?;
        self.word_view_by_id(&id.to_string()).await
    }
}

fn sense_kind(kind: voca_store::SenseKind) -> &'static str {
    match kind {
        voca_store::SenseKind::Word => "word",
        voca_store::SenseKind::Phrase => "phrase",
        voca_store::SenseKind::Example => "example",
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

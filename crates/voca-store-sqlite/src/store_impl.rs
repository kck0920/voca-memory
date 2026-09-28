use sqlx::{QueryBuilder, Sqlite};
use time::OffsetDateTime;
use voca_domain::{Id, ReviewState, SenseKind, SenseSource, WordSource, xp_progress};
use voca_store::{
    CardId, CardView, Change, ChangePage, ChangeRequest, Dashboard, DeckId, DeckProgress,
    DeckUpdate, DeckView, DeckViewCardRef, NewDeck, NewUserSense, Page, QueuedCard, ReviewOutcome,
    ReviewRequest, Revision, SenseId, SenseQuery, SenseView, Store, StoreError, StoreResult,
    StudyQueue, StudyRequest, UserId, Versioned, WordView,
};

// `DeckRow` / `SenseRow` / `CardRow` 라는 이름이 **DB 행**과 **동기화 DTO** 양쪽에 있다.
// 어느 쪽이 `Change` 안에 들어가는지 읽는 사람이 헷갈리지 않도록 별칭으로 드러낸다.
// 여기서 잘못 섞으면 클라이언트에 반쪽짜리 Card 가 나간다 — 그게 이 별칭의 이유다.
use crate::row::{DeckRow as DeckDb, SenseRow as SenseDb, StateRow as StateDb, WordRow as WordDb};
use crate::{SqliteStore, Tx, classify, now_unix_seconds, unix_seconds};
use voca_store::{
    CardRow as CardDto, DeckRow as DeckDto, SenseRow as SenseDto, StateRow as StateDto,
};

/// FSRS-6 파라미터 집합의 forgetting-curve decay. 기억 회수 가능성 계산에 쓴다.
///
/// Retention Preset이 바꾸는 것은 `desired_retention`이고 decay는 아니다 — decay는
/// 모델의 성질이지 사용자 설정이 아니다.
const FSRS_DECAY: f32 = fsrs::FSRS6_DEFAULT_DECAY;

impl Store for SqliteStore {
    // ── 사전 ────────────────────────────────────────────────

    async fn find_word(&self, user: UserId, lemma: &str) -> StoreResult<Option<WordView>> {
        // 사용자 단어를 먼저 찾는다. 조회를 목격한 사람이 방금 만든 그 단어일
        // 가능성이 높기 때문이다.
        //
        // `source = 'user'` 인 행은 **이 사용자의 것만** 본다. 다른 사용자의 개인
        // 뜻이 새어 나오면 안 된다. 사전 단어는 전역이라 누구에게나 보인다.
        let row: Option<WordDb> = sqlx::query_as(
            "SELECT id, lemma, source, phonetic, audio_url, rev FROM words
             WHERE lemma = ?1 COLLATE NOCASE
               AND (source = 'dictionary' OR user_id = ?2)
             ORDER BY source = 'user' DESC LIMIT 1",
        )
        .bind(lemma)
        .bind(user.to_string())
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;

        match row {
            Some(r) => self.word_view(r).await.map(Some),
            None => Ok(None),
        }
    }

    async fn search_senses(
        &self,
        user: UserId,
        query: &SenseQuery,
    ) -> StoreResult<Page<SenseView>> {
        query.validate()?;

        // 조건 개수가 쿼리마다 달라지므로 `QueryBuilder` 로 만든다. 문자열을 이어붙여
        // 쿼리를 조립하면 `sqlx` 가 인젝션 위험을 걸러 막는다 — 바인드 파라미터로
        // 넘길 수 있는 것은 전부 넘긴다. 종류 필터의 `IN (...)` 는 자릿수 자체가
        // 바뀌니 `separated` 로 처리한다.
        let term = query.text.clone().unwrap_or_default();
        let like = format!("%{term}%");
        let source: Option<&str> = match query.source {
            Some(true) => Some("user"),
            Some(false) => Some("dictionary"),
            None => None,
        };
        let word: Option<String> = query.word.map(|w| w.to_string());

        // **사전 뜻과 내 뜻만 본다.** 남의 개인 뜻이 검색에 나오면 안 된다.
        // `senses` 에는 `user_id` 가 없으므로 소속 Word 를 통해 가른다.
        let mut count = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(*) FROM senses WHERE archived_at IS NULL \
             AND word_id IN (SELECT id FROM words WHERE source = 'dictionary' OR user_id = ",
        );
        count.push_bind(user.to_string()).push(")");
        // `source` 가 `None` 이면 "전부" 다. 그런데 `AND source = NULL` 로 바인딩하면
        // SQL 에서 절대로 참이 되지 않아 **항상 0건**이 된다. 조건이 없을 때는
        // 절을 아예 내보내지 않는다.
        if let Some(s) = source {
            count.push(" AND source = ").push_bind(s);
        }
        if let Some(w) = &word {
            count.push(" AND word_id = ").push_bind(w);
        }
        if !term.trim().is_empty() {
            count
                .push(" AND (definition LIKE ")
                .push_bind(like.clone())
                .push(" COLLATE NOCASE OR word_id IN (SELECT id FROM words WHERE lemma LIKE ")
                .push_bind(like.clone())
                .push(" COLLATE NOCASE))");
        }
        if !query.kinds.is_empty() {
            count.push(" AND kind IN (");
            let mut sep = count.separated(", ");
            for k in &query.kinds {
                sep.push_bind(kind_name(*k));
            }
            count.push(")");
        }
        let total: i64 = count
            .build_query_scalar()
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;

        let mut list = QueryBuilder::<Sqlite>::new(
            "SELECT id, word_id, kind, source, pos, definition, example_en, example_ko, rev \
             FROM senses WHERE archived_at IS NULL \
             AND word_id IN (SELECT id FROM words WHERE source = 'dictionary' OR user_id = ",
        );
        list.push_bind(user.to_string()).push(")");
        if let Some(s) = source {
            list.push(" AND source = ").push_bind(s);
        }
        if let Some(w) = &word {
            list.push(" AND word_id = ").push_bind(w);
        }
        if !term.trim().is_empty() {
            list.push(" AND (definition LIKE ")
                .push_bind(like.clone())
                .push(" COLLATE NOCASE OR word_id IN (SELECT id FROM words WHERE lemma LIKE ")
                .push_bind(like.clone())
                .push(" COLLATE NOCASE))");
        }
        if !query.kinds.is_empty() {
            list.push(" AND kind IN (");
            let mut sep = list.separated(", ");
            for k in &query.kinds {
                sep.push_bind(kind_name(*k));
            }
            list.push(")");
        }
        list.push(" ORDER BY id LIMIT ")
            .push_bind(i64::from(query.limit));
        list.push(" OFFSET ").push_bind(i64::from(query.offset));

        let rows: Vec<SenseDb> = list
            .build_query_as()
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            items.push(self.sense_view(row).await?);
        }
        Ok(Page::new(items, clamp_u32(total)))
    }

    async fn put_user_sense(&self, user: UserId, input: NewUserSense) -> StoreResult<SenseView> {
        input.validate()?;
        let mut tx = self.begin().await?;
        let now = now_unix_seconds();

        let word_id = self
            .ensure_user_word(&mut tx, &user.to_string(), input.lemma.trim(), now)
            .await?;
        let id = new_id();

        sqlx::query(
            "INSERT INTO senses
                 (id, word_id, kind, source, pos, definition, example_en, updated_at, rev)
             VALUES (?1, ?2, ?3, 'user', ?4, ?5, ?6, ?7, 1)",
        )
        .bind(&id)
        .bind(&word_id)
        .bind(kind_name(input.kind))
        .bind(input.pos.as_deref().map(str::trim))
        .bind(input.definition.trim())
        .bind(input.example_en.as_deref().map(str::trim))
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(classify)?;

        tx.commit().await.map_err(classify)?;
        self.sense_view_by_id(&id).await
    }

    async fn archive_sense(&self, user: UserId, sense: SenseId) -> StoreResult<()> {
        let now = now_unix_seconds();
        // **소유권을 확인한다.** `senses` 에는 `user_id` 가 없다 — 아래의 `words`
        // 를 거쳐 이 사용자의 것인지 본다. 이 검사를 빼면 아무 Sense로나 숨길 수 있다.
        let n = sqlx::query(
            "UPDATE senses SET archived_at = ?3, rev = rev + 1, updated_at = ?3
             WHERE id = ?1 AND archived_at IS NULL
               AND word_id IN (SELECT id FROM words
                               WHERE user_id = ?2 OR source = 'dictionary')",
        )
        .bind(sense.to_string())
        .bind(user.to_string())
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected();

        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    // ── 덱 ──────────────────────────────────────────────────

    async fn list_decks(&self, user: UserId) -> StoreResult<Vec<DeckView>> {
        let rows = sqlx::query_as::<_, DeckDb>(
            "SELECT id, user_id, name, description, daily_goal, new_per_day, rev
             FROM decks WHERE user_id = ?1 AND deleted_at IS NULL
             -- created_at 은 초 단위라 같은 초에 만든 덱이 여럿이면 동률이다.
             -- rowid 는 SQLite 가 삽입 순서로 붙이는 정수라 그걸 정확히 대신한다.
             ORDER BY created_at, rowid",
        )
        .bind(user.to_string())
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(deck_view(row)?);
        }
        Ok(out)
    }

    async fn create_deck(&self, user: UserId, input: NewDeck) -> StoreResult<DeckView> {
        if input.name.trim().is_empty() {
            return Err(StoreError::Invalid("덱 이름이 비었다"));
        }
        let id = new_id();
        let now = now_unix_seconds();
        sqlx::query(
            "INSERT INTO decks
                 (id, user_id, name, description, daily_goal, new_per_day,
                  created_at, rev, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, 20), COALESCE(?6, 10), ?7, 1, ?7)",
        )
        .bind(&id)
        .bind(user.to_string())
        .bind(input.name.trim())
        .bind(input.description.as_deref().map(str::trim))
        .bind(input.daily_goal.map(i64::from))
        .bind(input.new_per_day.map(i64::from))
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(classify)?;

        self.deck_view(&parse_id(&id)?).await
    }

    async fn update_deck(&self, user: UserId, input: DeckUpdate) -> StoreResult<DeckView> {
        let n = sqlx::query(
            "UPDATE decks SET
                 name        = COALESCE(?2, name),
                 description = CASE WHEN ?3 = 1 THEN ?4 ELSE description END,
                 daily_goal  = COALESCE(?5, daily_goal),
                 new_per_day = COALESCE(?6, new_per_day),
                 rev = rev + 1, updated_at = ?7
             WHERE id = ?1 AND deleted_at IS NULL AND user_id = ?8",
        )
        .bind(input.id.to_string())
        .bind(input.name.as_deref().map(str::trim))
        .bind(i64::from(u8::from(input.description.is_some())))
        .bind(
            input
                .description
                .as_ref()
                .and_then(|d| d.as_deref())
                .map(str::trim),
        )
        .bind(input.daily_goal.map(i64::from))
        .bind(input.new_per_day.map(i64::from))
        .bind(now_unix_seconds())
        .bind(user.to_string())
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected();

        if n == 0 {
            return Err(StoreError::NotFound);
        }
        self.deck_view(&input.id).await
    }

    // ── 카드 ────────────────────────────────────────────────

    async fn add_cards(
        &self,
        user: UserId,
        deck: DeckId,
        senses: &[SenseId],
    ) -> StoreResult<Vec<CardView>> {
        let mut tx = self.begin().await?;
        let now = now_unix_seconds();

        // 덱 소유권을 먼저 확인한다. 남의 덱에 아무거나 못 넣는다.
        let owner: Option<String> =
            sqlx::query_scalar("SELECT user_id FROM decks WHERE id = ?1 AND deleted_at IS NULL")
                .bind(deck.to_string())
                .fetch_optional(&mut *tx)
                .await
                .map_err(classify)?;
        match owner.as_deref() {
            Some(u) if u == user.to_string() => {}
            _ => return Err(StoreError::NotFound),
        }

        let mut out = Vec::new();
        for sense in senses {
            let id = new_id();
            // 이미 있는 Sense 는 조용히 넘긴다. partial unique index 가 막아주지만
            // 에러로 실패시키는 것보다 "넣은 만큼" 돌려주는 편이 호출부에 낫다.
            // rev = 1 로 시작한다. **새로 만들어진 행은 한 번 "바뀐" 것이다.** 0 으로
            // 두면 동기화 트리거가 타지 않아, 나중에 첫 동기기를 돌린 클라이언트가
            // 이 Card 를 영영 못 받는다.
            let n = sqlx::query(
                "INSERT OR IGNORE INTO cards (id, user_id, deck_id, sense_id, rev, updated_at)
                 SELECT ?1, ?2, ?3, s.id, 1, ?4 FROM senses s
                 WHERE s.id = ?5 AND s.archived_at IS NULL",
            )
            .bind(&id)
            .bind(user.to_string())
            .bind(deck.to_string())
            .bind(now)
            .bind(sense.to_string())
            .execute(&mut *tx)
            .await
            .map_err(classify)?
            .rows_affected();
            if n == 0 {
                continue;
            }

            // 새 Card 는 지금부터 풀 수 있다.
            sqlx::query(
                "INSERT INTO card_states
                     (card_id, state, elapsed_days, scheduled_days, due_at,
                      reps, lapses, rev, updated_at)
                 VALUES (?1, 'new', 0, 0, ?2, 0, 0, 1, ?2)",
            )
            .bind(&id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;

            out.push(CardView {
                id: parse_id(&id)?,
                deck_id: deck,
                sense_id: *sense,
            });
        }

        tx.commit().await.map_err(classify)?;
        Ok(out)
    }

    async fn clone_card(
        &self,
        user: UserId,
        card: CardId,
        to_deck: DeckId,
    ) -> StoreResult<CardView> {
        let mut tx = self.begin().await?;
        let now = now_unix_seconds();

        // 원본 Card 는 내 것이어야 한다.
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT user_id, deck_id, sense_id FROM cards
             WHERE id = ?1 AND deleted_at IS NULL AND user_id = ?2",
        )
        .bind(card.to_string())
        .bind(user.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify)?;
        let Some((user_id, _, sense_id)) = row else {
            return Err(StoreError::NotFound);
        };

        // **대상 덱도 내 것이어야 한다.** 이 검사를 빼면 남의 덱에 내 Card 를 심을 수
        // 있어, 덱별 집계와 동기화가 서로 다른 사람의 데이터를 섞는다.
        let deck_owner: Option<String> =
            sqlx::query_scalar("SELECT user_id FROM decks WHERE id = ?1 AND deleted_at IS NULL")
                .bind(to_deck.to_string())
                .fetch_optional(&mut *tx)
                .await
                .map_err(classify)?;
        match deck_owner.as_deref() {
            Some(u) if u == user.to_string() => {}
            _ => return Err(StoreError::NotFound),
        }

        // 여러 덱에 넣으려면 복제한다 (docs/adr/0004). 복제본은 복습 이력을
        // **가져오지 않는다** — `cloned_from` 만 원본을 가리킨다.
        let id = new_id();
        let n = sqlx::query(
            "INSERT OR IGNORE INTO cards
                 (id, user_id, deck_id, sense_id, cloned_from, rev, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
        )
        .bind(&id)
        .bind(&user_id)
        .bind(to_deck.to_string())
        .bind(&sense_id)
        .bind(card.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(classify)?
        .rows_affected();
        if n == 0 {
            return Err(StoreError::Integrity);
        }

        sqlx::query(
            "INSERT INTO card_states
                 (card_id, state, elapsed_days, scheduled_days, due_at,
                  reps, lapses, rev, updated_at)
             VALUES (?1, 'new', 0, 0, ?2, 0, 0, 1, ?2)",
        )
        .bind(&id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(classify)?;

        tx.commit().await.map_err(classify)?;
        Ok(CardView {
            id: parse_id(&id)?,
            deck_id: to_deck,
            sense_id: parse_id(&sense_id)?,
        })
    }

    async fn archive_card(&self, user: UserId, card: CardId) -> StoreResult<()> {
        let now = now_unix_seconds();
        // **소유권을 확인한다.** 없으면 남의 Card 를 아무 Card 처럼 지울 수 있다.
        let n = sqlx::query(
            "UPDATE cards SET deleted_at = ?3, rev = rev + 1, updated_at = ?3
             WHERE id = ?1 AND deleted_at IS NULL AND user_id = ?2",
        )
        .bind(card.to_string())
        .bind(user.to_string())
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected();
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    // ── 복습 ────────────────────────────────────────────────

    async fn study_queue(&self, request: StudyRequest) -> StoreResult<StudyQueue> {
        let user = request.user.to_string();
        let deck_filter: Option<String> = request.deck.map(|d| d.to_string());

        let rows: Vec<StateDb> = sqlx::query_as(
            "SELECT cs.card_id, cs.state, cs.stability, cs.difficulty, cs.elapsed_days,
                    cs.scheduled_days, cs.due_at, cs.last_review_at, cs.introduced_at,
                    cs.reps, cs.lapses, cs.rev
             FROM card_states cs
             JOIN cards c ON c.id = cs.card_id
             WHERE c.user_id = ?1 AND c.deleted_at IS NULL
               AND (?2 IS NULL OR c.deck_id = ?2)",
        )
        .bind(&user)
        .bind(&deck_filter)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;

        let new_budget = self
            .new_budget(&user, deck_filter.as_deref(), request.now)
            .await?;

        let mut fresh: Vec<QueuedCard> = Vec::new();
        let mut due: Vec<QueuedCard> = Vec::new();
        for row in &rows {
            let state = row.to_memory_state()?;
            if !state.is_due(request.now) {
                continue;
            }
            let queued = self.queued_card(row, request.now).await?;
            if state.state == ReviewState::New {
                fresh.push(queued);
            } else {
                due.push(queued);
            }
        }

        // 복습은 회수 가능성이 낮은 순 — 가장 잊을 것 같은 단어가 먼저 나온다.
        // `total_cmp` 는 NaN 까지 전순서를 정의하므로 비교가 흔들리지 않는다.
        due.sort_by(|a, b| a.retrievability.total_cmp(&b.retrievability));
        // 신규는 회수 가능성이 모두 1.0 이다. 정렬 기준이 없으므로 id로 고정한다 —
        // 실행마다 다른 순서가 나오면 테스트도 사용자도 혼란스러워진다.
        fresh.sort_by_key(|c| c.card.card_id);

        // 신규 몫도 `limit` 안에 있어야 한다. `new_per_day` 가 크다고 해서 사용자가
        // 2 개를 요청했는데 3 개를 내밀면 안 된다.
        let capacity = request.limit as usize;
        let fresh_take = (new_budget as usize).min(fresh.len()).min(capacity);
        let reviews_wanted = capacity.saturating_sub(fresh_take);

        let reviews_remaining = due.len().saturating_sub(reviews_wanted);
        let fresh_remaining = fresh.len().saturating_sub(fresh_take);

        let mut cards: Vec<QueuedCard> = fresh.drain(..fresh_take).collect();
        cards.extend(due.into_iter().take(reviews_wanted));

        Ok(StudyQueue {
            cards,
            reviews_remaining: clamp_u32(reviews_remaining as i64),
            new_remaining_today: clamp_u32(fresh_remaining as i64),
        })
    }

    async fn submit_review(&self, request: ReviewRequest) -> StoreResult<ReviewOutcome> {
        SqliteStore::submit_review(self, request).await
    }

    // ── 대시보드 ────────────────────────────────────────────

    async fn dashboard(&self, user: UserId, now: OffsetDateTime) -> StoreResult<Dashboard> {
        let user_id = user.to_string();
        let now_unix = unix_seconds(now);
        let today_start = unix_seconds(now.date().midnight().assume_utc());

        let decks = self.list_decks(user).await?;

        let reviews_due: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM card_states cs
             JOIN cards c ON c.id = cs.card_id
             WHERE c.user_id = ?1 AND c.deleted_at IS NULL
               AND cs.state = 'review' AND cs.due_at <= ?2",
        )
        .bind(&user_id)
        .bind(now_unix)
        .fetch_one(self.pool())
        .await
        .map_err(classify)?;

        // 덱별 신규 한도를 모아 오늘 분량의 합을 낸다.
        let mut new_remaining: i64 = 0;
        let mut progress = Vec::with_capacity(decks.len());
        for deck in &decks {
            let introduced_today: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM card_states cs
                 JOIN cards c ON c.id = cs.card_id
                 WHERE c.deck_id = ?1 AND c.deleted_at IS NULL
                   AND cs.introduced_at IS NOT NULL AND cs.introduced_at >= ?2",
            )
            .bind(deck.id.to_string())
            .bind(today_start)
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;

            // **예산이 아니라 실제로 풀 수 있는 수**를 보고해야 한다. 오늘 몫이 남았다는
            // 이유만으로 "할 일이 있다"고 하면, 전부 복습한 뒤에도 대시보드가 일을
            // 안 난다고 말한다.
            let budget = i64::from(deck.new_per_day).saturating_sub(introduced_today);
            let available: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cards c
                 JOIN card_states cs ON cs.card_id = c.id
                 WHERE c.deck_id = ?1 AND c.deleted_at IS NULL AND cs.state = 'new'",
            )
            .bind(deck.id.to_string())
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;

            new_remaining += budget.min(available);
            progress.push(self.deck_progress(deck, now_unix).await?);
        }

        let total_xp: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(amount), 0) FROM xp_events WHERE user_id = ?1")
                .bind(&user_id)
                .fetch_one(self.pool())
                .await
                .map_err(classify)?;

        Ok(Dashboard {
            as_of: now,
            local_date: now.date(),
            streak: self.current_streak(&user_id).await?,
            level: xp_progress(u64::try_from(total_xp).unwrap_or(0)),
            reviews_due: clamp_u32(reviews_due),
            new_remaining: clamp_u32(new_remaining),
            decks: progress,
        })
    }

    // ── 동기화 ──────────────────────────────────────────────

    async fn changes_since(&self, request: ChangeRequest) -> StoreResult<ChangePage> {
        // limit 보다 한 개 더 읽어 "더 있다"를 판정한다.
        let limit = request.limit.max(1);
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT entity, entity_id, rev, deleted
             FROM change_log WHERE user_id = ?1 AND rev > ?2
             ORDER BY rev, id LIMIT ?3",
        )
        .bind(request.user.to_string())
        .bind(request.since.as_u64() as i64)
        .bind(i64::from(limit) + 1)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;

        let has_more = rows.len() > limit as usize;
        let mut watermark = request.since;
        let mut changes = Vec::new();

        for (entity, entity_id, rev, deleted) in rows.iter().take(limit as usize) {
            let revision = voca_store::Revision::from((*rev).max(0) as u64);
            // 갱신 안 된 엔티티는 갱신한 수와 같거나 작다. 클라이언트는 가장 큰
            // rev 만 알면 되므로 항상 덮어쓴다.
            if revision > watermark {
                watermark = revision;
            }
            let id = entity_id.clone();

            changes.push(match (entity.as_str(), *deleted != 0) {
                (_, true) => match entity.as_str() {
                    "card" => Change::CardTombstone {
                        id: parse_id(&id)?,
                        revision,
                    },
                    "deck" => Change::DeckTombstone {
                        id: parse_id(&id)?,
                        revision,
                    },
                    _ => Change::SenseTombstone {
                        id: parse_id(&id)?,
                        revision,
                    },
                },
                ("deck", false) => match self.deck_dto(&id).await? {
                    Some(dto) => Change::Deck(Versioned::new(dto, revision)),
                    None => continue,
                },
                ("card", false) => match self.card_row(&id).await? {
                    Some(row) => Change::Card(Versioned::new(row, revision)),
                    None => continue,
                },
                (_, false) => match self.sense_dto(&id).await? {
                    Some(dto) => Change::Sense(Versioned::new(dto, revision)),
                    None => Change::SenseTombstone {
                        id: parse_id(&id)?,
                        revision,
                    },
                },
            });
        }

        Ok(ChangePage {
            changes,
            watermark,
            has_more,
        })
    }
}

// ── 행 → 보기 변환과 파생값 ──────────────────────────────

impl SqliteStore {
    async fn word_view(&self, row: WordDb) -> StoreResult<WordView> {
        let senses = sqlx::query_as::<_, SenseDb>(
            "SELECT id, word_id, kind, source, pos, definition, example_en, example_ko, rev
             FROM senses WHERE word_id = ?1 AND archived_at IS NULL ORDER BY id",
        )
        .bind(&row.id)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;

        let mut views = Vec::with_capacity(senses.len());
        for s in senses {
            views.push(self.sense_view(s).await?);
        }

        Ok(WordView::new(
            parse_id(&row.id)?,
            row.lemma,
            parse_word_source(&row.source),
            row.phonetic,
            row.audio_url,
            views,
        ))
    }

    async fn sense_view(&self, row: SenseDb) -> StoreResult<SenseView> {
        let in_decks: Vec<String> = sqlx::query_scalar(
            "SELECT deck_id FROM cards WHERE sense_id = ?1 AND deleted_at IS NULL",
        )
        .bind(&row.id)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;

        Ok(SenseView {
            id: parse_id(&row.id)?,
            word_id: parse_id(&row.word_id)?,
            kind: parse_kind(&row.kind),
            source: parse_sense_source(&row.source),
            pos: row.pos,
            definition: row.definition,
            example_en: row.example_en,
            example_ko: row.example_ko,
            in_decks: in_decks
                .iter()
                .map(|d| parse_id(d))
                .collect::<StoreResult<Vec<_>>>()?,
        })
    }

    async fn sense_view_by_id(&self, id: &str) -> StoreResult<SenseView> {
        let row: Option<SenseDb> = sqlx::query_as(
            "SELECT id, word_id, kind, source, pos, definition, example_en, example_ko, rev
             FROM senses WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;

        match row {
            Some(r) => self.sense_view(r).await,
            None => Err(StoreError::NotFound),
        }
    }

    async fn deck_view(&self, id: &DeckId) -> StoreResult<DeckView> {
        match self.deck_row(&id.to_string()).await? {
            Some(row) => deck_view(row),
            None => Err(StoreError::NotFound),
        }
    }

    async fn deck_row(&self, id: &str) -> StoreResult<Option<DeckDb>> {
        sqlx::query_as(
            "SELECT id, user_id, name, description, daily_goal, new_per_day, rev
             FROM decks WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)
    }

    async fn deck_dto(&self, id: &str) -> StoreResult<Option<DeckDto>> {
        let Some(r) = self.deck_row(id).await? else {
            return Ok(None);
        };
        Ok(Some(DeckDto {
            id: parse_id(&r.id)?,
            name: r.name,
            description: r.description,
            daily_goal: clamp_u32(r.daily_goal),
            new_per_day: clamp_u32(r.new_per_day),
        }))
    }

    async fn sense_dto(&self, id: &str) -> StoreResult<Option<SenseDto>> {
        let row: Option<SenseDb> = sqlx::query_as(
            "SELECT id, word_id, kind, source, pos, definition, example_en, example_ko, rev
             FROM senses WHERE id = ?1 AND archived_at IS NULL",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;

        match row {
            Some(r) => Ok(Some(SenseDto {
                id: parse_id(&r.id)?,
                word_id: parse_id(&r.word_id)?,
                kind: r.kind,
                source: r.source,
                pos: r.pos,
                definition: r.definition,
                example_en: r.example_en,
                example_ko: r.example_ko,
            })),
            None => Ok(None),
        }
    }

    /// Card 본문과 스케줄러 상태를 한 번에 읽는다. 동기화가 둘을 한 단위로
    /// 전달해야 하는 이유다.
    async fn card_row(&self, id: &str) -> StoreResult<Option<CardDto>> {
        let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, deck_id, sense_id, cloned_from FROM cards
             WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;

        let state: Option<StateDb> = sqlx::query_as(
            "SELECT card_id, state, stability, difficulty, elapsed_days, scheduled_days,
                    due_at, last_review_at, introduced_at, reps, lapses, rev
             FROM card_states WHERE card_id = ?1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;

        match (row, state) {
            (Some((cid, deck_id, sense_id, cloned_from)), Some(cs)) => Ok(Some(CardDto {
                id: parse_id(&cid)?,
                deck_id: parse_id(&deck_id)?,
                sense_id: parse_id(&sense_id)?,
                cloned_from: match cloned_from {
                    Some(c) => Some(parse_id(&c)?),
                    None => None,
                },
                state: StateDto {
                    state: cs.state,
                    stability: cs.stability,
                    difficulty: cs.difficulty,
                    elapsed_days: cs.elapsed_days,
                    scheduled_days: cs.scheduled_days,
                    due_at: cs.due_at,
                    last_review_at: cs.last_review_at,
                    reps: clamp_u32(cs.reps),
                    lapses: clamp_u32(cs.lapses),
                },
            })),
            _ => Ok(None),
        }
    }

    /// 이 사용자의 Word 를 찾거나 만든다.
    ///
    /// **사용자별로 나눈다.** 표기형만으로 찾으면 두 사람이 같은 표기형의 뜻을 만들
    /// 때 한 Word 를 공유해, A의 개인 뜻이 B의 조회에 나타난다. 그건
    /// `words_user_lemma` 인덱스도 (사용자, 표기형) 으로 잡혀 있어야 일관적이다.
    async fn ensure_user_word(
        &self,
        tx: &mut Tx,
        user: &str,
        lemma: &str,
        now: i64,
    ) -> StoreResult<String> {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM words
             WHERE lemma = ?1 COLLATE NOCASE AND source = 'user' AND user_id = ?2",
        )
        .bind(lemma)
        .bind(user)
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify)?;
        if let Some(id) = existing {
            return Ok(id);
        }

        let id = new_id();
        sqlx::query(
            "INSERT INTO words (id, lemma, source, user_id, updated_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
        )
        .bind(&id)
        .bind(lemma)
        .bind(user)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(classify)?;
        Ok(id)
    }

    /// 오늘 더 소개할 수 있는 신규 Card 수.
    async fn new_budget(
        &self,
        user: &str,
        deck: Option<&str>,
        now: OffsetDateTime,
    ) -> StoreResult<u32> {
        let today_start = unix_seconds(now.date().midnight().assume_utc());

        let row: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT d.id, d.new_per_day,
                    (SELECT COUNT(*) FROM card_states cs
                       JOIN cards c ON c.id = cs.card_id
                      WHERE c.deck_id = d.id AND c.deleted_at IS NULL
                        AND cs.introduced_at IS NOT NULL AND cs.introduced_at >= ?3)
             FROM decks d
             WHERE d.user_id = ?1 AND d.deleted_at IS NULL AND (?2 IS NULL OR d.id = ?2)",
        )
        .bind(user)
        .bind(deck)
        .bind(today_start)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;

        Ok(clamp_u32(
            row.iter()
                .map(|(_, limit, used)| (limit - used).max(0))
                .sum(),
        ))
    }

    async fn queued_card(&self, row: &StateDb, now: OffsetDateTime) -> StoreResult<QueuedCard> {
        let card: Option<(String, String)> =
            sqlx::query_as("SELECT deck_id, sense_id FROM cards WHERE id = ?1")
                .bind(&row.card_id)
                .fetch_optional(self.pool())
                .await
                .map_err(classify)?;
        let Some((deck_id, sense_id)) = card else {
            return Err(StoreError::NotFound);
        };

        let sense = self.sense_view_by_id(&sense_id).await?;
        let word = self.word_view_by_id(&sense.word_id.to_string()).await?;
        let memory_state = row.to_memory_state()?;

        Ok(QueuedCard {
            card: DeckViewCardRef {
                card_id: parse_id(&row.card_id)?,
                deck_id: parse_id(&deck_id)?,
            },
            retrievability: retrievability(&memory_state, now),
            word,
            sense,
            memory_state,
        })
    }

    pub(crate) async fn word_view_by_id(&self, id: &str) -> StoreResult<WordView> {
        let row: Option<WordDb> = sqlx::query_as(
            "SELECT id, lemma, source, phonetic, audio_url, rev FROM words WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;
        match row {
            Some(r) => self.word_view(r).await,
            None => Err(StoreError::NotFound),
        }
    }

    async fn current_streak(&self, user: &str) -> StoreResult<voca_domain::Streak> {
        let row: Option<crate::row::StreakRow> = sqlx::query_as(
            "SELECT current_count, longest_count, last_review_date FROM streaks WHERE user_id = ?1",
        )
        .bind(user)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;
        Ok(row.map_or_else(voca_domain::Streak::default, |r| r.to_streak()))
    }

    async fn deck_progress(&self, deck: &DeckView, now_unix: i64) -> StoreResult<DeckProgress> {
        let (total, seen, due, fresh): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
                 COUNT(*),
                 COALESCE(SUM(cs.reps > 0), 0),
                 COALESCE(SUM(cs.state = 'review' AND cs.due_at <= ?2), 0),
                 COALESCE(SUM(cs.state = 'new'), 0)
             FROM cards c
             JOIN card_states cs ON cs.card_id = c.id
             WHERE c.deck_id = ?1 AND c.deleted_at IS NULL",
        )
        .bind(deck.id.to_string())
        .bind(now_unix)
        .fetch_one(self.pool())
        .await
        .map_err(classify)?;

        Ok(DeckProgress {
            id: deck.id,
            total: clamp_u32(total),
            seen: clamp_u32(seen),
            due: clamp_u32(due),
            fresh: clamp_u32(fresh),
        })
    }
}

// ── 순수 함수 ────────────────────────────────────────────

/// Card의 현재 기억 회수 가능성(0~1). 낮을수록 잊을 가능성이 크다.
fn retrievability(state: &voca_domain::MemoryState, now: OffsetDateTime) -> f32 {
    let Some(memory) = state.memory else {
        return 1.0;
    };
    let elapsed = state
        .last_review_at
        .map(|last| (now - last).as_seconds_f32().max(0.0) / 86_400.0)
        .unwrap_or(0.0);

    fsrs::current_retrievability(
        fsrs::MemoryState {
            stability: memory.stability.max(0.1),
            difficulty: memory.difficulty,
        },
        elapsed,
        FSRS_DECAY,
    )
}

/// `id` 파싱은 실패할 수 있으므로 `DeckView` 를 곧바로 돌려줄 수 없다. 이 저장소가
/// 직접 만든 UUID 라 실제로는 실패하지 않지만, 실패를 조용히 삼키면 다른 DB 를 붙였을
/// 때 0 번 식별자로 조용히 진행된다.
fn deck_view(row: DeckDb) -> StoreResult<DeckView> {
    Ok(DeckView {
        id: parse_id(&row.id)?,
        name: row.name,
        description: row.description,
        daily_goal: clamp_u32(row.daily_goal),
        new_per_day: clamp_u32(row.new_per_day),
        revision: Revision::from(row.rev.max(0) as u64),
    })
}

fn kind_name(kind: SenseKind) -> &'static str {
    match kind {
        SenseKind::Word => "word",
        SenseKind::Phrase => "phrase",
        SenseKind::Example => "example",
    }
}

fn parse_kind(raw: &str) -> SenseKind {
    match raw {
        "phrase" => SenseKind::Phrase,
        "example" => SenseKind::Example,
        _ => SenseKind::Word,
    }
}

fn parse_word_source(raw: &str) -> WordSource {
    match raw {
        "user" => WordSource::User,
        _ => WordSource::Dictionary,
    }
}

fn parse_sense_source(raw: &str) -> SenseSource {
    match raw {
        "user" => SenseSource::User,
        _ => SenseSource::Dictionary,
    }
}

fn clamp_u32(value: i64) -> u32 {
    value.clamp(0, i64::from(u32::MAX)) as u32
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// DB에서 읽은 문자열을 `Id`로 되돌린다.
///
/// 이 저장소가 직접 만든 UUID 라 실패하지 않는다. 그래도 실패를 무시하지 않는다 —
/// 스키마가 다른 DB를 붙였을 때 여기서 조용히 0번 식별자가 나오면 안 된다.
pub(crate) fn parse_id(raw: &str) -> StoreResult<Id> {
    Id::parse(raw).map_err(|_| StoreError::Integrity)
}

-- Voca Memory 초기 스키마.
-- 설계 근거는 docs/design.md 의 "데이터 모델" 절.

PRAGMA foreign_keys = ON;

-- 계정 ────────────────────────────────────────────────
CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name  TEXT NOT NULL,
    -- IANA 시간대. **표시 전용이다.** Streak 판정에는 쓰지 않는다 — 여행 중 접속하면
    -- 이미 기록된 날짜가 달라져 버리기 때문이다. docs/design.md "Streak" 참조.
    timezone      TEXT NOT NULL,
    -- Retention Preset: diligent | balanced | frugal
    retention     TEXT NOT NULL DEFAULT 'balanced',
    created_at    INTEGER NOT NULL
) STRICT;

-- 전역 어휘 사전 ────────────────────────────────────────
-- source = 'dictionary' 행은 앱이 쓰지 않는다. 임포트와 API 캐시로만 생긴다.
CREATE TABLE words (
    id         TEXT PRIMARY KEY,
    lemma      TEXT NOT NULL,
    source     TEXT NOT NULL,
    -- `source = 'user'` 일 때만 채워지고, `source = 'dictionary'` 이면 NULL이다.
    --
    -- **이게 없으면 개인 뜻이 계정 간에 새어 나간다.** 표기형만으로 사용자 Word를
    -- 찾으면 두 사람이 같은 표기형의 뜻을 만들 때 한 Word를 공유하게 되어, A의 개인
    -- 뜻이 B의 `find_word` 에 나타난다. 사용자 Word는 **사용자마다** 따로이다.
    user_id    TEXT REFERENCES users(id) ON DELETE CASCADE,
    phonetic   TEXT,
    audio_url  TEXT,
    rev        INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX words_dictionary_lemma
    ON words (lemma COLLATE NOCASE) WHERE source = 'dictionary';
-- 사용자 Word는 (사용자, 표기형) 쌍으로 유일하다. 표기형만으로 잡지 않는다.
CREATE UNIQUE INDEX words_user_lemma
    ON words (user_id, lemma COLLATE NOCASE) WHERE source = 'user';

CREATE TABLE senses (
    id          TEXT PRIMARY KEY,
    -- kind = 'example'여도 NOT NULL이다. 그 Sense는 설명되는 원래 단어에 속하고
    -- Card 앞면으로 example_en을 보여준다. docs/adr/0011.
    word_id     TEXT NOT NULL REFERENCES words(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    source      TEXT NOT NULL,
    pos         TEXT,
    definition  TEXT NOT NULL,
    example_en  TEXT,
    example_ko  TEXT,
    archived_at INTEGER,
    rev         INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX senses_word ON senses (word_id);
CREATE INDEX senses_lookup ON senses (kind, source) WHERE archived_at IS NULL;

-- 사용자 소유 학습 재료 ─────────────────────────────────
CREATE TABLE decks (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    daily_goal  INTEGER NOT NULL DEFAULT 20,
    new_per_day INTEGER NOT NULL DEFAULT 10,
    -- 덱 목록의 순서. 사용자가 만든 순서를 유지한다.
    created_at  INTEGER NOT NULL,
    rev         INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL,
    deleted_at  INTEGER
) STRICT;

CREATE INDEX decks_user ON decks (user_id) WHERE deleted_at IS NULL;

-- Card는 정확히 하나의 Deck에 속한다. 여러 덱에 넣으려면 복제한다.
-- docs/adr/0004.
CREATE TABLE cards (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    deck_id     TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
    sense_id    TEXT NOT NULL REFERENCES senses(id) ON DELETE CASCADE,
    cloned_from TEXT REFERENCES cards(id) ON DELETE SET NULL,
    rev         INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL,
    deleted_at  INTEGER
) STRICT;

CREATE INDEX cards_deck ON cards (deck_id) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX cards_deck_sense
    ON cards (deck_id, sense_id) WHERE deleted_at IS NULL;

-- 스케줄러 상태. Card와 1:1이며 본문과 분리된다.
-- state는 new | review 두 값뿐이다. 순수 FSRS-6은 learning step을 모델링하지 않는다.
-- docs/adr/0009.
CREATE TABLE card_states (
    card_id        TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    state          TEXT NOT NULL,
    stability      REAL,
    difficulty     REAL,
    elapsed_days   REAL NOT NULL DEFAULT 0,
    scheduled_days REAL NOT NULL DEFAULT 0,
    due_at         INTEGER NOT NULL,
    last_review_at INTEGER,
    -- 첫 복습 시각. 아직 한 번도 복습하지 않았으면 NULL.
    -- `new_per_day` 제한은 "오늘 몇 개를 **처음** 봤는가"로 세어야 하는데,
    -- `last_review_at`로는 그릴 수 없다 (오래된 단어를 오늘 다시 보면 갱신된다).
    -- 그래서 도입 시각을 따로 둔다.
    introduced_at  INTEGER,
    reps           INTEGER NOT NULL DEFAULT 0,
    lapses         INTEGER NOT NULL DEFAULT 0,
    rev            INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL
) STRICT;

-- Due 조회와 회수 가능성 정렬이 여기서 나간다.
CREATE INDEX card_states_due ON card_states (due_at);
CREATE INDEX card_states_review_due ON card_states (state, due_at) WHERE state = 'review';

CREATE TABLE review_log (
    id             TEXT PRIMARY KEY,
    user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    card_id        TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    rating         INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 4),
    reviewed_at    INTEGER NOT NULL,
    -- 세션 동안 고정된 오프셋으로 환산한 현지 날짜. Streak 판정 기준.
    local_date     TEXT NOT NULL,
    duration_ms    INTEGER,
    prev_stability REAL,
    next_stability REAL,
    rev            INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX review_log_user_date ON review_log (user_id, local_date);
CREATE INDEX review_log_card ON review_log (card_id, reviewed_at);

-- review_log에서 파생한 캐시. Review가 쌓일 때 함께 갱신한다.
CREATE TABLE streaks (
    user_id          TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    current_count    INTEGER NOT NULL DEFAULT 0,
    longest_count    INTEGER NOT NULL DEFAULT 0,
    last_review_date TEXT,
    rev              INTEGER NOT NULL DEFAULT 0,
    updated_at       INTEGER NOT NULL
) STRICT;

-- append-only. review_log에서 파생하므로 rev이 없다.
CREATE TABLE xp_events (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    amount      INTEGER NOT NULL,
    source_id   TEXT,
    occurred_at INTEGER NOT NULL
) STRICT;

CREATE INDEX xp_events_user ON xp_events (user_id, occurred_at);

-- 동기화용. 어느 테이블에서든 "rev 이후"를 훑을 수 있어야 한다.
CREATE TABLE change_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    TEXT NOT NULL,
    entity     TEXT NOT NULL,
    entity_id  TEXT NOT NULL,
    rev        INTEGER NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX change_log_user_rev ON change_log (user_id, rev);

-- 동기화 이력. **INSERT 와 UPDATE 를 모두 기록한다.** UPDATE 트리거만 두면 새로 만든
-- 덱·카드가 목록에 아예 없으므로, 새 클라이언트의 첫 동기화가 빈손이 된다.
-- 한 행이 두 번 기록되지 않도록 각 트리거는 자기 행의 rev 증가에만 반응한다.
CREATE TRIGGER decks_insert AFTER INSERT ON decks
WHEN NEW.rev > 0
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    VALUES (NEW.user_id, 'deck', NEW.id, NEW.rev, 0, NEW.updated_at);
END;

CREATE TRIGGER decks_touch AFTER UPDATE ON decks
WHEN NEW.rev > OLD.rev
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    VALUES (NEW.user_id, 'deck', NEW.id, NEW.rev,
            CASE WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END,
            NEW.updated_at);
END;

CREATE TRIGGER cards_insert AFTER INSERT ON cards
WHEN NEW.rev > 0
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    VALUES (NEW.user_id, 'card', NEW.id, NEW.rev, 0, NEW.updated_at);
END;

CREATE TRIGGER cards_touch AFTER UPDATE ON cards
WHEN NEW.rev > OLD.rev
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    VALUES (NEW.user_id, 'card', NEW.id, NEW.rev,
            CASE WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END,
            NEW.updated_at);
END;

-- Card의 스케줄러 상태가 바뀌면 Card 자체가 바뀐 것으로 기록한다. INSERT 쪽 트리거는
-- 두지 않는다 — Card 가 만들어질 때 이미 본문과 상태가 함께 기록되기 때문이다. 클라이언트가
-- Card 본문과 상태를 한 단위로 받게 하려고다. `card_state` 를 별도 엔티티로 두면
-- 클라이언트가 둘을 따로 적용하다 도중에 중간 상태를 보게 된다.
CREATE TRIGGER card_states_touch AFTER UPDATE ON card_states
WHEN NEW.rev > OLD.rev
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    SELECT c.user_id, 'card', NEW.card_id, MAX(NEW.rev, c.rev), 0, NEW.updated_at
    FROM cards c WHERE c.id = NEW.card_id;
END;

-- 전역 사전이라 user_id 가 없다. 동기화 대상이 아니고 임포트 경로가 관리한다.
CREATE TRIGGER senses_insert AFTER INSERT ON senses
WHEN NEW.rev > 0
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    VALUES ('', 'sense', NEW.id, NEW.rev, 0, NEW.updated_at);
END;

CREATE TRIGGER senses_touch AFTER UPDATE ON senses
WHEN NEW.rev > OLD.rev
BEGIN
    INSERT INTO change_log (user_id, entity, entity_id, rev, deleted, created_at)
    VALUES ('', 'sense', NEW.id, NEW.rev,
            CASE WHEN NEW.archived_at IS NOT NULL THEN 1 ELSE 0 END,
            NEW.updated_at);
END;

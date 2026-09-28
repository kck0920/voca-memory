-- 로그인 세션.
--
-- **메모리에 두지 않는다.** 프로세스를 재시작하면 전원이 로그아웃된다. 배포 대상이
-- 한 대의 SQLite 파일이므로 (docs/adr/0010) 여기 두는 게 가장 단순하다.
--
-- **토큰을 해시해서 저장한다.** 평문으로 두면 DB 를 유출한 사람이 곧바로 세션
-- 탈취한다. 비밀번호와 같은 취급이다.
CREATE TABLE sessions (
    -- sha256 토큰 해시의 16바이트. 토큰 자체는 브라우저에만 있다.
    token_hash BLOB PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    -- 마지막 사용 시각. 만료 계산에 쓴다.
    seen_at    INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX sessions_user ON sessions (user_id);
CREATE INDEX sessions_expiry ON sessions (expires_at);

-- 새 세션을 만들 때 만료된 것을 치운다. `sessions_expiry` 인덱스가 있어 값싸다.
-- 백그라운드 작업 대신 이쪽이 더 단순하고, 삽입이 드물어 비용도 무시할 만하다.
CREATE TRIGGER sessions_purge AFTER INSERT ON sessions
BEGIN
    DELETE FROM sessions WHERE expires_at < unixepoch();
END;

-- 외부 사전 API 응답 캐시.
--
-- **API 를 다시 부르지 않게 하는 것이 목적이다.** 호출한 횟수만큼 지연이 생기고,
-- 공개 API 는 호출 한도가 있다. 캐시가 없으면 같은 단어를 두 번째로 추가할 때
-- 또 기다려야 한다.
--
-- 응답을 원문 그대로 둔다. 파싱 규칙이 나중에 바뀌면 캐시된 원문으로 다시 파싱해
-- 값을 살릴 수 있다. 파싱된 결과를 저장하면 그게 불가능하다.
CREATE TABLE dict_cache (
    lemma       TEXT PRIMARY KEY COLLATE NOCASE,
    source      TEXT NOT NULL,
    body        TEXT NOT NULL,
    status      INTEGER NOT NULL,   -- 200 = 성공, 404 = 없음, 그 외 = 오류
    fetched_at  INTEGER NOT NULL,
    -- 0 이면 영원히. 성공한 항목은 유한 기간을 두지 않는다 — 사전 뜻은 안 바뀐다.
    -- 실패한 항목만 짧게 둔다.
    expires_at  INTEGER
) STRICT;

CREATE INDEX dict_cache_expiry ON dict_cache (expires_at)
    WHERE expires_at IS NOT NULL;

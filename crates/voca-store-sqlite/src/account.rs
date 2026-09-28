use voca_store::{Accounts, Credential, NewAccount, SessionGrant, StoreError, StoreResult};

use crate::SqliteStore;

use voca_store::DEFAULT_RETENTION;

/// `retention` 은 자유 문자열이라 DB에 들어가기 전에 좁힌다. 스키마 CHECK 제약에
/// 기대면 오류 메시지가 DB 구현에 새고, 무엇이 잘못됐는지 알 수 없다.
fn normalize_retention(raw: Option<&str>) -> &'static str {
    match raw {
        Some("diligent") => "diligent",
        Some("frugal") => "frugal",
        _ => DEFAULT_RETENTION,
    }
}

impl Accounts for SqliteStore {
    async fn create(&self, input: NewAccount) -> StoreResult<SessionGrant> {
        let now = crate::now_unix_seconds();
        let retention = normalize_retention(input.retention.as_deref());

        sqlx::query(
            "INSERT INTO users
                 (id, email, password_hash, display_name, timezone, retention, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(input.id.to_string())
        .bind(input.email.trim().to_lowercase())
        .bind(&input.password_hash)
        .bind(input.display_name.trim())
        .bind(input.timezone.trim())
        .bind(retention)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(|e| {
            // UNIQUE 위반은 "이미 쓰이는 이메일"이라는 뜻이다. 존재 여부를 그대로
            // 노출하므로 로그인 쪽도 같은 메시지를 쓰도록 규칙이 맞춰져 있다.
            let duplicate = matches!(&e, sqlx::Error::Database(db) if db.is_unique_violation());
            if duplicate {
                StoreError::Integrity
            } else {
                crate::classify(e)
            }
        })?;

        Ok(SessionGrant {
            user_id: input.id,
            display_name: input.display_name.trim().to_owned(),
            retention: retention.to_owned(),
        })
    }

    async fn find_by_email(&self, email: &str) -> StoreResult<Option<Credential>> {
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, password_hash, display_name, retention, created_at
             FROM users WHERE email = ?1 COLLATE NOCASE",
        )
        .bind(email.trim())
        .fetch_optional(self.pool())
        .await
        .map_err(crate::classify)?;

        let Some((id, password_hash, display_name, retention, created_at)) = row else {
            return Ok(None);
        };

        Ok(Some(Credential {
            user_id: voca_domain::Id::parse(&id).map_err(|_| StoreError::Integrity)?,
            password_hash,
            display_name,
            retention,
            created_at,
        }))
    }

    async fn find_by_id(&self, id: voca_store::UserId) -> StoreResult<Option<Credential>> {
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, password_hash, display_name, retention, created_at
             FROM users WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(self.pool())
        .await
        .map_err(crate::classify)?;

        let Some((user_id, password_hash, display_name, retention, created_at)) = row else {
            return Ok(None);
        };
        Ok(Some(Credential {
            user_id: voca_domain::Id::parse(&user_id).map_err(|_| StoreError::Integrity)?,
            password_hash,
            display_name,
            retention,
            created_at,
        }))
    }

    async fn count(&self) -> StoreResult<u64> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(self.pool())
            .await
            .map_err(crate::classify)?;
        Ok(n.max(0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use voca_store::UserId;

    async fn store() -> (TempDir, SqliteStore) {
        let dir = TempDir::new().unwrap();
        let s = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();
        (dir, s)
    }

    fn account(id: u8, email: &str) -> NewAccount {
        NewAccount {
            id: UserId::from([id; 16]),
            email: email.into(),
            password_hash: "$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA".into(),
            display_name: "테스터".into(),
            timezone: "Asia/Seoul".into(),
            retention: None,
        }
    }

    #[tokio::test]
    async fn a_new_account_lands_with_the_default_retention() {
        let (_d, s) = store().await;
        let grant = s.create(account(1, "a@b.kr")).await.unwrap();
        assert_eq!(grant.retention, "balanced");
        assert_eq!(grant.display_name, "테스터");
        assert_eq!(s.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn emails_are_matched_case_insensitively_and_stored_lowercase() {
        let (_d, s) = store().await;
        s.create(account(1, "Someone@Example.KR")).await.unwrap();

        let found = s.find_by_email("someone@example.kr").await.unwrap();
        assert!(found.is_some(), "대소문자만 다른 이메일을 못 찾았다");

        let upper = sqlx::query_scalar::<_, String>("SELECT email FROM users")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(upper, "someone@example.kr", "소문자로 정규화되지 않았다");
    }

    #[tokio::test]
    async fn surrounding_whitespace_is_trimmed_before_matching() {
        let (_d, s) = store().await;
        s.create(account(1, "a@b.kr")).await.unwrap();
        assert!(s.find_by_email("  a@b.kr  ").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_duplicate_email_is_refused() {
        let (_d, s) = store().await;
        s.create(account(1, "a@b.kr")).await.unwrap();
        let second = s.create(account(2, "A@B.KR")).await;
        assert_eq!(second, Err(StoreError::Integrity));
    }

    #[tokio::test]
    async fn a_missing_account_is_not_found_not_an_error() {
        let (_d, s) = store().await;
        assert_eq!(s.find_by_email("nobody@nowhere.kr").await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_account_can_be_found_by_id() {
        // 세션이 살아 있을 때 현재 사용자를 되살리는 경로다. 이게 없으면
        // 표시 이름을 잃어서 빈 이름이 응답으로 나간다.
        let (_d, s) = store().await;
        let grant = s.create(account(1, "a@b.kr")).await.unwrap();
        let found = s.find_by_id(grant.user_id).await.unwrap().unwrap();
        assert_eq!(found.display_name, "테스터");
        assert_eq!(found.retention, "balanced");
    }

    #[tokio::test]
    async fn an_unknown_id_finds_nothing() {
        let (_d, s) = store().await;
        assert!(
            s.find_by_id(UserId::from([7u8; 16]))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_explicit_retention_is_kept() {
        let (_d, s) = store().await;
        let mut a = account(1, "a@b.kr");
        a.retention = Some("frugal".into());
        let grant = s.create(a).await.unwrap();
        assert_eq!(grant.retention, "frugal");
    }

    #[tokio::test]
    async fn an_unknown_retention_falls_back_to_the_default() {
        // retention 은 자유 문자열이라 들어올 때마다 좁혀야 한다. 미지의 값이
        // 그대로 저장되면 스케줄러가 나중에 조용히 기준값으로 떨어뜨린다.
        let (_d, s) = store().await;
        let mut a = account(1, "a@b.kr");
        a.retention = Some("집중".into());
        let grant = s.create(a).await.unwrap();
        assert_eq!(grant.retention, "balanced");
    }
}

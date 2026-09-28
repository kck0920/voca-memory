//! 세션 저장소 통합 테스트.
//!
//! 여기서 확인하는 것은 보안 불변식이다. "로그인 된다"만으로는 부족하다.

use tempfile::TempDir;
use time::OffsetDateTime;
use voca_store::{Sessions, UserId};

use voca_store_sqlite::SqliteStore;

async fn world() -> (TempDir, SqliteStore, UserId) {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();
    let user = UserId::from([1u8; 16]);

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'a@b.kr', 'x', '테스터', 'Asia/Seoul', 0)",
    )
    .bind(user.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    (dir, store, user)
}

#[tokio::test]
async fn a_fresh_token_resolves_to_its_user() {
    let (_d, store, user) = world().await;
    let token = store.create(user, 86_400).await.unwrap();
    assert_eq!(store.resolve(&token).await.unwrap(), Some(user));
}

#[tokio::test]
async fn an_unknown_token_resolves_to_nothing() {
    let (_d, store, _user) = world().await;
    let stranger = voca_store::SessionToken::new("0".repeat(64));
    assert_eq!(store.resolve(&stranger).await.unwrap(), None);
}

#[tokio::test]
async fn the_plain_token_is_never_stored() {
    // DB 를 유출한 사람이 토큰을 그대로 쓸 수 없어야 한다.
    let (_d, store, user) = world().await;
    let token = store.create(user, 86_400).await.unwrap();
    let plain = token.expose().to_string();

    // 해시 바이트를 그대로 꺼내 평문 토큰과 대조한다. 해시 함수가 값을 섞어 놓으므로
    // 두 문자열은 같을 수 없다.
    let raw: Vec<Vec<u8>> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert_eq!(raw.len(), 1, "세션이 저장되지 않았다");

    let as_hex: String = raw[0].iter().map(|b| format!("{b:02x}")).collect();
    assert_ne!(as_hex, plain, "평문 토큰이 그대로 저장됐다");
    assert_eq!(raw[0].len(), 32, "sha256 해시 길이가 아니다");
    assert!(
        !raw[0]
            .windows(plain.len() / 2)
            .any(|w| w == &plain.as_bytes()[..w.len()]),
        "평문 토큰의 조각이 저장됐다"
    );
}

#[tokio::test]
async fn revoking_a_token_kills_it_immediately() {
    let (_d, store, user) = world().await;
    let token = store.create(user, 86_400).await.unwrap();
    assert!(store.resolve(&token).await.unwrap().is_some());

    store.revoke(&token).await.unwrap();
    assert_eq!(store.resolve(&token).await.unwrap(), None);
}

#[tokio::test]
async fn revoking_twice_is_not_an_error() {
    // 로그아웃은 재요청될 수 있다. 두 번째가 실패하면 UI 가 오류를 띄운다.
    let (_d, store, user) = world().await;
    let token = store.create(user, 86_400).await.unwrap();
    store.revoke(&token).await.unwrap();
    assert!(store.revoke(&token).await.is_ok());
}

#[tokio::test]
async fn revoke_all_for_ends_every_device() {
    let (_d, store, user) = world().await;
    let phone = store.create(user, 86_400).await.unwrap();
    let laptop = store.create(user, 86_400).await.unwrap();

    store.revoke_all_for(user).await.unwrap();

    assert_eq!(store.resolve(&phone).await.unwrap(), None);
    assert_eq!(store.resolve(&laptop).await.unwrap(), None);
}

#[tokio::test]
async fn revoking_all_for_one_user_leaves_others_alone() {
    let (_d, store, user) = world().await;
    let other = UserId::from([9u8; 16]);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'b@b.kr', 'x', '다른 사람', 'Asia/Seoul', 0)",
    )
    .bind(other.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    let mine = store.create(user, 86_400).await.unwrap();
    let theirs = store.create(other, 86_400).await.unwrap();

    store.revoke_all_for(user).await.unwrap();

    assert_eq!(store.resolve(&mine).await.unwrap(), None);
    assert_eq!(
        store.resolve(&theirs).await.unwrap(),
        Some(other),
        "남의 세션까지 끊었다"
    );
}

#[tokio::test]
async fn an_expired_token_does_not_resolve() {
    // 최소 ttl 이 60초라 Past 를 만들려면 시계를 직접 옮겨야 한다.
    let (_d, store, user) = world().await;
    let token = store.create(user, 60).await.unwrap();

    sqlx::query("UPDATE sessions SET expires_at = ?1")
        .bind(OffsetDateTime::now_utc().unix_timestamp() - 1)
        .execute(store.pool())
        .await
        .unwrap();

    assert_eq!(store.resolve(&token).await.unwrap(), None);
}

#[tokio::test]
async fn a_nonsense_ttl_does_not_create_an_already_dead_session() {
    // ttl 이 0 이거나 음수면 "지금 만든 유효 세션"이 될 위험이 있다. 최소값을 둔다.
    let (_d, store, user) = world().await;
    for ttl in [0, -100] {
        let token = store.create(user, ttl).await.unwrap();
        assert!(
            store.resolve(&token).await.unwrap().is_some(),
            "ttl={ttl} 로 만든 세션이 바로 죽었다"
        );
        store.revoke(&token).await.unwrap();
    }
}

#[tokio::test]
async fn purging_removes_only_expired_sessions() {
    let (_d, store, user) = world().await;
    let live = store.create(user, 86_400).await.unwrap();
    let dying = store.create(user, 86_400).await.unwrap();

    // 토큰 해시는 알 수 없으므로 행 순서로 하나 골라 만료시킨다.
    // SQLite 는 `UPDATE ... ORDER BY ... LIMIT` 을 기본 빌드에서 받지 않는다.
    // 서브쿼리로 rowid 를 골라 준다. 나중에 만든 `dying` 를 만료시키려면 DESC 다.
    sqlx::query(
        "UPDATE sessions SET expires_at = expires_at - 1000000
         WHERE rowid = (SELECT rowid FROM sessions ORDER BY rowid DESC LIMIT 1)",
    )
    .execute(store.pool())
    .await
    .unwrap();

    let purged = store.purge_expired().await.unwrap();
    assert_eq!(purged, 1, "만료된 세션이 지워지지 않았다");
    assert!(store.resolve(&live).await.unwrap().is_some());
    assert_eq!(
        store.resolve(&dying).await.unwrap(),
        None,
        "이미 지워졌어야 하는 세션이다"
    );
}

#[tokio::test]
async fn creating_a_session_sweeps_out_dead_ones() {
    // 정리 작업을 별도로 돌리지 않아도 오래된 세션이 쌓이지 않는다. `sessions_expiry`
    // 인덱스가 있어 삽입마다 지워도 값싸다.
    let (_d, store, user) = world().await;
    for _ in 0..3 {
        store.create(user, 86_400).await.unwrap();
    }
    sqlx::query("UPDATE sessions SET expires_at = expires_at - 1000000")
        .execute(store.pool())
        .await
        .unwrap();

    store.create(user, 86_400).await.unwrap();

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(remaining, 1, "만료된 세션이 남아 있다");
}

#[tokio::test]
async fn two_users_never_share_a_token() {
    let (_d, store, a) = world().await;
    let b = UserId::from([2u8; 16]);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
         VALUES (?1, 'b@b.kr', 'x', 'B', 'Asia/Seoul', 0)",
    )
    .bind(b.to_string())
    .execute(store.pool())
    .await
    .unwrap();

    let token_a = store.create(a, 86_400).await.unwrap();
    let token_b = store.create(b, 86_400).await.unwrap();
    assert_ne!(token_a.expose(), token_b.expose());

    assert_eq!(store.resolve(&token_a).await.unwrap(), Some(a));
    assert_eq!(store.resolve(&token_b).await.unwrap(), Some(b));
}

#[tokio::test]
async fn sessions_survive_a_restart() {
    // 한 대의 SQLite 파일에 세션이 있으므로 (docs/adr/0010) 프로세스를 다시 띄워도
    // 로그인이 유지된다. 이게 안 되면 배포마다 전원이 로그아웃된다.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("v.db");
    let user = UserId::from([1u8; 16]);

    let token = {
        let store = SqliteStore::open(&path).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, timezone, created_at)
             VALUES (?1, 'a@b.kr', 'x', '테스터', 'Asia/Seoul', 0)",
        )
        .bind(user.to_string())
        .execute(store.pool())
        .await
        .unwrap();
        store.create(user, 86_400).await.unwrap()
    };

    let reopened = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        reopened.resolve(&token).await.unwrap(),
        Some(user),
        "재시작 후 세션이 사라졌다"
    );
}

#[tokio::test]
async fn deleting_a_user_deletes_their_sessions() {
    // 로그아웃했는데 세션 토큰으로 계속 들어올 수 있으면 안 된다.
    let (_d, store, user) = world().await;
    let token = store.create(user, 86_400).await.unwrap();

    sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(user.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    assert_eq!(store.resolve(&token).await.unwrap(), None);
}

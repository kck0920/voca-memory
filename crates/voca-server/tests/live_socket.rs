//! 실제 소켓을 거친 왕복.
//!
//! 나머지 HTTP 테스트는 라우터를 `oneshot` 으로 부른다 — 빠르고 결정적이지만
//! **HTTP 프레이밍을 우회한다.** 상태 줄, 헤더 구분을 직접 만들지 않으므로
//! 응답을 조립하는 코드가 깨져도 잡지 못한다.
//!
//! 여기서는 소켓을 열고 원시 HTTP/1.1 을 써서 확인한다. 규칙이나 배선이 아니라
//! 전송 계층만 본다.

use std::net::SocketAddr;
use std::time::Duration;

use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use voca_server::{AppState, Config, build_router};
use voca_store_sqlite::SqliteStore;

struct Live {
    _dir: TempDir,
    addr: SocketAddr,
}

impl Live {
    async fn start() -> Self {
        let dir = TempDir::new().unwrap();
        let store = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();

        let config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            database_path: dir.path().join("v.db").to_string_lossy().into_owned(),
            allowed_origins: vec!["https://voca.example.kr".into()],
            secure_cookies: false,
        };

        let app = build_router(AppState::new(store, &config));
        let listener = tokio::net::TcpListener::bind(config.bind).await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
        });

        Self { _dir: dir, addr }
    }

    /// 원시 HTTP/1.1 요청을 보내고 전체 응답을 문자열로 받는다.
    async fn raw(&self, request: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            // `Connection: close` 를 보냈으므로 서버가 닫으면 끝난다.
            stream.read_to_end(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        })
        .await
        .expect("응답이 오지 않았다")
    }
}

fn request(method: &str, path: &str, origin: Option<&str>, body: Option<&str>) -> String {
    let mut out =
        format!("{method} {path} HTTP/1.1\r\nHost: voca.example.kr\r\nConnection: close\r\n");
    if let Some(o) = origin {
        out.push_str(&format!("Origin: {o}\r\n"));
    }
    match body {
        Some(b) => {
            out.push_str("Content-Type: application/json\r\n");
            out.push_str(&format!("Content-Length: {}\r\n\r\n", b.len()));
            out.push_str(b);
        }
        None => out.push_str("Content-Length: 0\r\n\r\n"),
    }
    out
}

fn registration_json() -> &'static str {
    r#"{"email":"live@vocamemory.kr","password":"부지런한-비밀번호","display_name":"실거래","timezone":"Asia/Seoul","retention":null}"#
}

#[tokio::test]
async fn health_answers_over_a_real_socket() {
    let live = Live::start().await;
    let response = live.raw(&request("GET", "/api/health", None, None)).await;

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("\r\n\r\nok"), "본문이 잘렸다: {response}");
}

#[tokio::test]
async fn the_response_headers_are_framed_correctly() {
    // 상태 줄, 헤더, 빈 줄, 본문 — 이 순서가 틀리면 모든 HTTP 클라이언트가 실패한다.
    let live = Live::start().await;
    let response = live
        .raw(&request(
            "POST",
            "/api/auth/register",
            Some("https://voca.example.kr"),
            Some(registration_json()),
        ))
        .await;

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("헤더와 본문이 구분되지 않는다");
    assert!(
        head.contains("\r\nset-cookie:"),
        "쿠키 헤더가 없다:\n{head}"
    );
    // 길이는 반드시 알려야 한다. 비어 있으면 클라이언트가 연결을 닫아야 다음
    // 응답을 받을 수 있다.
    let declared: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .expect("Content-Length 가 없다");
    assert_eq!(
        declared,
        body.len(),
        "알린 길이와 실제 길이가 다르다 — 프레이밍이 깨졌을 수 있다"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(body).is_ok(),
        "본문이 JSON 이 아니다: {body}"
    );
}

#[tokio::test]
async fn a_cookie_from_one_response_works_on_the_next() {
    // 실제 클라이언트가 하는 그대로: Set-Cookie 를 저장하고 Cookie 로 되보낸다.
    let live = Live::start().await;

    let registered = live
        .raw(&request(
            "POST",
            "/api/auth/register",
            Some("https://voca.example.kr"),
            Some(registration_json()),
        ))
        .await;

    let set_cookie = registered
        .lines()
        .find_map(|l| l.strip_prefix("set-cookie: "))
        .or_else(|| {
            registered
                .lines()
                .find(|l| l.to_lowercase().starts_with("set-cookie:"))
                .map(|l| l.split_once(':').unwrap().1.trim_start())
        })
        .expect("Set-Cookie 가 없다");
    let pair = set_cookie.split(';').next().unwrap().to_owned();

    let me = live
        .raw(&request_with_header(
            "GET",
            "/api/auth/me",
            &format!("Cookie: {pair}\r\n"),
        ))
        .await;

    assert!(me.starts_with("HTTP/1.1 200 OK"), "{me}");
    let (_, body) = me.split_once("\r\n\r\n").unwrap();
    let json: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
    assert_eq!(json["display_name"], "실거래");
}

fn request_with_header(method: &str, path: &str, extra: &str) -> String {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: voca.example.kr\r\nConnection: close\r\n{extra}Content-Length: 0\r\n\r\n"
    )
}

#[tokio::test]
async fn a_foreign_origin_is_refused_over_the_wire_too() {
    let live = Live::start().await;
    let response = live
        .raw(&request(
            "POST",
            "/api/auth/register",
            Some("https://evil.kr"),
            Some(registration_json()),
        ))
        .await;
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "타 출처가 통과했다:\n{response}"
    );
    assert!(!response.contains("set-cookie"), "차단했는데 쿠키를 줬다");
}

#[tokio::test]
async fn an_unknown_route_is_404_not_a_hang() {
    let live = Live::start().await;
    let response = live.raw(&request("GET", "/없는-경로", None, None)).await;
    assert!(response.starts_with("HTTP/1.1 404"), "{response}");
}

#[tokio::test]
async fn a_malformed_request_line_does_not_kill_the_server() {
    // 나쁜 요청이 들어오면 그 연결만 죽어야 한다. 서버가 죽으면 배포 후 복구가
    // 필요해진다.
    let live = Live::start().await;
    let _ = live.raw("완전히 잘못된 요청\r\n\r\n").await;

    // 그 다음 요청이 아직 살아 있어야 한다.
    let after = live.raw(&request("GET", "/api/health", None, None)).await;
    assert!(
        after.starts_with("HTTP/1.1 200 OK"),
        "나쁜 요청이 서버를 죽였다: {after}"
    );
}

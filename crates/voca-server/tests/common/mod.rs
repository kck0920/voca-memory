//! HTTP 테스트 공용 헬퍼.
//!
//! 소켓을 띄우지 않고 라우터를 **직접** 부른다. 실제 TCP 왕복은 배포 전에 한 번
//! 확인하면 되고, 그 위의 배선을 굳이 다시 확인할 이유는 없다. TCP 는 신뢰하되
//! 그 위의 배선은 직접 본다.

#![allow(dead_code)]

use std::net::SocketAddr;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;
use voca_server::{AppState, Config, build_router};
use voca_store_sqlite::SqliteStore;

/// 테스트 대상 서버.
pub struct Harness {
    _dir: TempDir,
    app: axum::Router,
    pub store: SqliteStore,
}

impl Harness {
    pub async fn start() -> Self {
        Self::with_origins(&["https://voca.example.kr"]).await
    }

    pub async fn with_origins(origins: &[&str]) -> Self {
        Self::full(origins, None).await
    }

    /// 외부 사전의 주소까지 지정한다. `None` 이면 실제 주소 — 네트워크를 보지
    /// 않는 테스트에서는 이걸 쓰지 않는다.
    pub async fn with_dictionary(dictionary_endpoint: &str) -> Self {
        Self::full(&["https://voca.example.kr"], Some(dictionary_endpoint)).await
    }

    pub async fn full(origins: &[&str], dictionary_endpoint: Option<&str>) -> Self {
        let dir = TempDir::new().unwrap();
        let store = SqliteStore::open(&dir.path().join("v.db")).await.unwrap();

        let config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            database_path: dir.path().join("v.db").to_string_lossy().into_owned(),
            allowed_origins: origins.iter().map(|s| (*s).to_owned()).collect(),
            // 테스트는 HTTP 라서 Secure 를 끈다. 붙이는 것도 따로 시험한다.
            secure_cookies: false,
        };

        let mut state = AppState::new(store.clone(), &config);
        if let Some(endpoint) = dictionary_endpoint {
            state = state.with_dictionary_endpoint(endpoint);
        }
        Self {
            _dir: dir,
            app: build_router(state),
            store,
        }
    }

    /// 요청 하나를 보낸다.
    pub async fn send(&self, request: Request<Body>) -> Reply {
        // ConnectInfo 를 넣어 둔다. 안 넣으면 어댑터가 IP 축 없이 동작한다.
        let (mut parts, body) = request.into_parts();
        parts
            .extensions
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                40_000,
            ))));

        let response = self
            .app
            .clone()
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("라우터가 응답하지 못했다");

        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_owned(),
                    v.to_str().unwrap_or_default().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("본문을 읽지 못했다")
            .to_bytes();

        Reply {
            status,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    pub async fn get(&self, path: &str, cookie: Option<&str>) -> Reply {
        let mut req = Request::builder().uri(path);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        self.send(req.body(Body::empty()).unwrap()).await
    }

    pub async fn post(&self, path: &str, origin: Option<&str>, json: serde_json::Value) -> Reply {
        self.post_with_cookie(path, origin, None, json).await
    }

    /// 로그아웃처럼 **지금 로그인한 사람의 쿠키가 필요한** 요청.
    pub async fn post_with_cookie(
        &self,
        path: &str,
        origin: Option<&str>,
        cookie: Option<&str>,
        json: serde_json::Value,
    ) -> Reply {
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        self.send(req.body(Body::from(json.to_string())).unwrap())
            .await
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// `Set-Cookie` 에서 `name=value` 만 떼어낸다. 속성은 그대로 둬야 동작을 알 수 있다.
    pub fn cookie(&self) -> Option<(String, String)> {
        let raw = self.header("set-cookie")?;
        let first = raw.split(';').next()?;
        let (name, value) = first.split_once('=')?;
        Some((name.trim().to_owned(), value.trim().to_owned()))
    }

    /// 요청에 실어 보낼 `Cookie` 헤더 값.
    pub fn cookie_header(&self) -> Option<String> {
        self.cookie().map(|(n, v)| format!("{n}={v}"))
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }

    pub fn code(&self) -> String {
        self.json()
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    }
}

pub fn registration(email: &str) -> serde_json::Value {
    serde_json::json!({
        "email": email,
        "password": "부지런한-비밀번호",
        "display_name": "테스터",
        "timezone": "Asia/Seoul",
        "retention": null,
    })
}

pub fn credentials(email: &str, password: &str) -> serde_json::Value {
    serde_json::json!({ "email": email, "password": password })
}

pub const ORIGIN: &str = "https://voca.example.kr";

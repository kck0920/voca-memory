//! 서버 설정.
//!
//! 환경변수에서만 읽는다. 설정 파일 형식과 유효성 검사 층은 개인 프로젝트에는
//! 과하다. **기본값을 정할 수 없는 것만 필수로 둔다.**

use std::net::SocketAddr;

/// 설정 누락이나 잘못된 값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub field: &'static str,
    pub detail: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} 설정이 잘못됐다: {}", self.field, self.detail)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 바인드 주소. 기본은 127.0.0.1 — **0.0.0.0 이 아니다.** 로컬에서 개발하다
    /// 실수로 외부에 노출되는 것보다, 배포자가 명시적으로 여는 편이 낫다.
    pub bind: SocketAddr,
    pub database_path: String,
    /// CSRF 검사에서 신뢰하는 출처 목록. **비어 있으면 아무것도 신뢰하지 않는다.**
    pub allowed_origins: Vec<String>,
    /// `true` 면 세션 쿠키에 `Secure` 를 붙인다. HTTP 로 개발할 때만 끈다.
    pub secure_cookies: bool,
}

impl Config {
    /// 환경변수에서 읽는다.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok().filter(|v| !v.trim().is_empty()))
    }

    /// 값 찾기를 주입받아 읽는다.
    ///
    /// 환경변수를 직접 읽으면 테스트가 프로세스 전역 상태에 의존해 병렬 실행에서
    /// 깨진다. 조회 함수를 넘기면 테스트가 순수해진다.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let read = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let port = read("VOCA_PORT")
            .unwrap_or_else(|| "3000".to_owned())
            .parse::<u16>()
            .map_err(|e| ConfigError {
                field: "VOCA_PORT",
                detail: format!("{e}"),
            })?;

        let host = read("VOCA_HOST").unwrap_or_else(|| "127.0.0.1".to_owned());
        let bind: SocketAddr = format!("{host}:{port}").parse().map_err(|e| ConfigError {
            field: "VOCA_HOST/VOCA_PORT",
            detail: format!("{e}"),
        })?;

        // 데이터베이스 파일 위치는 필수다. 기본값을 두면 개발용 DB가 실수로 프로덕션에
        // 쓰이거나, 그 반대다.
        let database_path = read("VOCA_DB_PATH").ok_or(ConfigError {
            field: "VOCA_DB_PATH",
            detail: "SQLite 파일 경로가 필요하다".into(),
        })?;

        let allowed_origins = read("VOCA_ALLOWED_ORIGINS")
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let secure_cookies = read("VOCA_SECURE_COOKIES")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);

        Ok(Self {
            bind,
            database_path,
            allowed_origins,
            secure_cookies,
        })
    }

    /// CSRF 가 실제로 작동하는 배치인지 확인한다.
    ///
    /// `allowed_origins` 가 비어 있으면 모든 변경 요청이 거부된다 — 즉 서비스가
    /// **완전히 죽는다.** 그게 조용한 것보다 낫지만, 배포자가 알아야 한다.
    pub fn csrf_would_reject_everything(&self) -> bool {
        self.allowed_origins.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    fn minimal() -> Vec<(&'static str, &'static str)> {
        vec![("VOCA_DB_PATH", "/tmp/voca.db")]
    }

    #[test]
    fn a_database_path_is_required() {
        // 기본값을 두면 개발용 DB가 실수로 프로덕션에 쓰이거나 그 반대다.
        let e = Config::from_lookup(env(&[])).unwrap_err();
        assert_eq!(e.field, "VOCA_DB_PATH");
    }

    #[test]
    fn a_database_path_alone_is_enough() {
        let c = Config::from_lookup(env(&minimal())).unwrap();
        assert_eq!(c.database_path, "/tmp/voca.db");
    }

    #[test]
    fn the_default_bind_is_loopback_not_everywhere() {
        // 로컬에서 개발하다 실수로 외부에 노출되는 것보다, 배포자가 명시적으로
        // 여는 편이 낫다.
        let c = Config::from_lookup(env(&minimal())).unwrap();
        assert_eq!(c.bind.to_string(), "127.0.0.1:3000");
        assert!(
            !c.bind.ip().is_unspecified(),
            "0.0.0.0 이 기본값이면 안 된다"
        );
    }

    #[test]
    fn the_port_can_be_changed() {
        let mut e = minimal();
        e.push(("VOCA_PORT", "8080"));
        assert_eq!(Config::from_lookup(env(&e)).unwrap().bind.port(), 8080);
    }

    #[test]
    fn a_non_numeric_port_is_rejected_naming_the_field() {
        let mut e = minimal();
        e.push(("VOCA_PORT", "http"));
        let err = Config::from_lookup(env(&e)).unwrap_err();
        assert!(err.to_string().contains("VOCA_PORT"), "{err}");
    }

    #[test]
    fn origins_are_split_on_commas_and_trimmed() {
        let mut e = minimal();
        e.push((
            "VOCA_ALLOWED_ORIGINS",
            "https://voca.example.kr, https://voca.example.com ,",
        ));
        let c = Config::from_lookup(env(&e)).unwrap();
        assert_eq!(
            c.allowed_origins,
            vec![
                "https://voca.example.kr".to_owned(),
                "https://voca.example.com".to_owned()
            ],
            "빈 조각이 남거나 공백이 붙었다"
        );
    }

    #[test]
    fn secure_cookies_default_on_and_can_be_turned_off_for_local_http() {
        assert!(Config::from_lookup(env(&minimal())).unwrap().secure_cookies);

        let mut e = minimal();
        e.push(("VOCA_SECURE_COOKIES", "0"));
        assert!(!Config::from_lookup(env(&e)).unwrap().secure_cookies);

        e = minimal();
        e.push(("VOCA_SECURE_COOKIES", "false"));
        assert!(!Config::from_lookup(env(&e)).unwrap().secure_cookies);
    }

    #[test]
    fn a_blank_value_counts_as_unset() {
        let mut e = minimal();
        e.push(("VOCA_ALLOWED_ORIGINS", "   "));
        let c = Config::from_lookup(env(&e)).unwrap();
        assert!(c.allowed_origins.is_empty());
    }

    #[test]
    fn an_empty_allow_list_would_block_everything() {
        // 설정이 빠졌을 때 CSRF 가 조용히 꺼지는 것이 가장 나쁜 실패다.
        let c = Config::from_lookup(env(&minimal())).unwrap();
        assert!(c.csrf_would_reject_everything());
    }

    #[test]
    fn a_populated_allow_list_works() {
        let mut e = minimal();
        e.push(("VOCA_ALLOWED_ORIGINS", "https://voca.example.kr"));
        let c = Config::from_lookup(env(&e)).unwrap();
        assert!(!c.csrf_would_reject_everything());
    }
}

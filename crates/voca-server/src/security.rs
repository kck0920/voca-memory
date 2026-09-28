use std::net::IpAddr;

use voca_store::AuthFailure;

/// CSRF 방어.
///
/// 쿠키에 `SameSite=Lax` 가 붙어 있어 **다른 사이트가 만든 form POST 는 쿠키를
/// 보내지 않는다.** 그래도 이것만 믿지 않는다. 브라우저가 아니라 다른 HTTP
/// 클라이언트는 `SameSite` 를 아예 지키지 않을 수 있고, `Sec-Fetch-Site` 도 없는
/// 클라이언트가 흔하다.
///
/// 그래서 **변경 요청에는 `Origin`(또는 `Referer`) 이 내 도메인이어야 한다** 는 것을
/// 직접 확인한다. CORS 기본값처럼 허용 도메인은 설정에서 온다 — 하드코딩하지 않는다.
pub fn origin_is_trusted(origin: Option<&str>, referer: Option<&str>, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        // 허용 목록이 없으면 아무것도 신뢰하지 않는다. 오开放로 두는 쪽이 낫다 —
        // 운영 설정이 빠졌을 때 CSRF 방어가 조용히 꺼지는 것은 가장 나쁜 실패다.
        return false;
    }

    let candidate = origin.or(referer);
    let Some(candidate) = candidate else {
        // 둘 다 없으면 신뢰하지 않는다. 브라우저는 보낸다.
        return false;
    };

    allowed.iter().any(|a| same_origin(a, candidate))
}

fn same_origin(allowed: &str, candidate: &str) -> bool {
    let (a_host, a_port) = split_authority(allowed);
    let (c_host, c_port) = split_authority(candidate);
    a_host.eq_ignore_ascii_case(&c_host) && effective_port(a_port) == effective_port(c_port)
}

/// URL 에서 authority(호스트와 포트)만 떼어낸다.
///
/// `Referer` 에는 경로가 붙어 있다(`https://voca.example.kr/x`). 경로를 떼어내지
/// 않으면 호스트 자리에 경로까지 들어가 비교가 깨진다.
fn split_authority(raw: &str) -> (String, Option<String>) {
    let without_scheme = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);

    // 경로·질의·조각은 여기서 떨어진다.
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme);

    match authority.rsplit_once(':') {
        // 포트 숫자가 있는 경우만 포트로 본다. "a.kr:8080" 과 "a.kr" 을 구분해야 한다.
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host.to_owned(), Some(port.to_owned()))
        }
        _ => (authority.to_owned(), None),
    }
}

/// HTTPS 면 443, HTTP 면 80, 명시하면 그 값을 쓴다.
fn effective_port(port: Option<String>) -> u16 {
    match port {
        Some(p) => p.parse().unwrap_or(if is_https() { 443 } else { 80 }),
        None => {
            if is_https() {
                443
            } else {
                80
            }
        }
    }
}

fn is_https() -> bool {
    // 역방향 프록시(HTTPS 종료) 뒤에서 돌아가는 게 기본 배치다. `X-Forwarded-Proto`
    // 를 프록시가 붙여 준다고 가정한다.
    std::env::var("VOCA_ASSUME_HTTPS")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(true)
}

/// 로그인 시도 제한.
///
/// 공개 배포라 브루트포스를 막을 수단이 필요하다. **전역 잠금**은 DoS에 약하므로
/// (공격자가 아는 계정을 잠글 수 있다) `(이메일 해시, IP)` 쌍마다 센다. 한쪽만
/// 잠그면 다른 쪽으로 우회할 수 있기 때문이다.
/// `(이메일, IP)` — 한쪽만 잠그면 다른 쪽으로 우회할 수 있다.
type ThrottleKey = (String, Option<IpAddr>);
/// `(지금까지의 실패 횟수, 마지막 시도 시각)`
type ThrottleEntry = (u32, i64);

#[derive(Debug, Default)]
pub struct LoginThrottle {
    inner: std::sync::Mutex<std::collections::HashMap<ThrottleKey, ThrottleEntry>>,
}

impl LoginThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    /// 시도가 너무 많으면 `AuthFailure` 를 돌려준다.
    ///
    /// `now` 은 unix 시각. 테스트에서 시간을 통제하려고 넘긴다.
    pub fn check(&self, email: &str, ip: Option<IpAddr>, now: i64) -> Result<(), AuthFailure> {
        let key = (email.trim().to_lowercase(), ip);
        let mut map = match self.inner.lock() {
            Ok(m) => m,
            // 잠금에 실패하면 **통과시킨다.** 잠금 경합으로 로그인을 막는 것은
            // 서비스 거부보다 나쁘다.
            Err(_) => return Ok(()),
        };

        // 오래된 항목을 먼저 걷어내 무한 증가를 막는다.
        if map.len() > 10_000 {
            let cutoff = now - WINDOW_SECONDS * 2;
            map.retain(|_, (_, seen)| *seen > cutoff);
        }

        let entry = map.entry(key).or_insert((0, now));
        if now - entry.1 > WINDOW_SECONDS {
            *entry = (0, now);
        }

        if entry.0 >= MAX_ATTEMPTS {
            return Err(AuthFailure::TooManyAttempts);
        }
        entry.0 += 1;
        Ok(())
    }

    /// 로그인에 성공하면 카운터를 지운다. 진짜 사용자가 맞다는 뜻이다.
    pub fn record_success(&self, email: &str, ip: Option<IpAddr>) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(&(email.trim().to_lowercase(), ip));
        }
    }
}

const WINDOW_SECONDS: i64 = 15 * 60;
const MAX_ATTEMPTS: u32 = 8;

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(hosts: &[&str]) -> Vec<String> {
        hosts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_matching_origin_is_trusted() {
        let a = allowed(&["https://voca.example.kr"]);
        assert!(origin_is_trusted(Some("https://voca.example.kr"), None, &a));
        assert!(origin_is_trusted(
            Some("https://voca.example.kr:443"),
            None,
            &a
        ));
    }

    #[test]
    fn a_foreign_origin_is_not_trusted() {
        let a = allowed(&["https://voca.example.kr"]);
        assert!(!origin_is_trusted(Some("https://evil.kr"), None, &a));
        assert!(!origin_is_trusted(
            Some("https://voca.example.kr.evil.kr"),
            None,
            &a
        ));
        // 서브도메인도 다르다.
        assert!(!origin_is_trusted(
            Some("https://x.voca.example.kr"),
            None,
            &a
        ));
    }

    #[test]
    fn a_missing_origin_is_not_trusted() {
        // 브라우저는 Origin 을 보내지만, 붙여넣기 든 스크립트는 안 보낼 수 있다.
        // 그 requests 는 거부해야 한다.
        let a = allowed(&["https://voca.example.kr"]);
        assert!(!origin_is_trusted(None, None, &a));
    }

    #[test]
    fn referer_is_used_when_origin_is_absent() {
        // Referer 에는 경로가 붙어 온다. 경로를 떼어내지 않으면 안 통과한다.
        let a = allowed(&["https://voca.example.kr"]);
        assert!(origin_is_trusted(
            None,
            Some("https://voca.example.kr/x"),
            &a
        ));
        assert!(origin_is_trusted(
            None,
            Some("https://voca.example.kr/deep/path?q=1#frag"),
            &a
        ));
        assert!(!origin_is_trusted(None, Some("https://evil.kr/x"), &a));
    }

    #[test]
    fn a_path_on_the_same_host_is_still_the_same_origin() {
        // 규격상 Origin 에는 경로가 없다. 규격을 어긴 값이 들어와도 **비교하는 것은
        // authority** 이고, 같은 호스트면 같은 출처다. 경로가 붙었다고 거절하면
        // 규격 밖의 정직한 클라이언트까지 막는다.
        let a = allowed(&["https://voca.example.kr"]);
        assert!(origin_is_trusted(
            Some("https://voca.example.kr/evil.kr:8080"),
            None,
            &a
        ));
    }

    #[test]
    fn an_empty_allow_list_trusts_nothing() {
        // 설정이 빠졌을 때 CSRF 방어가 조용히 꺼지는 것이 가장 나쁜 실패다.
        assert!(!origin_is_trusted(
            Some("https://voca.example.kr"),
            None,
            &[]
        ));
    }

    #[test]
    fn a_different_port_is_a_different_origin() {
        let a = allowed(&["https://voca.example.kr:8443"]);
        assert!(origin_is_trusted(
            Some("https://voca.example.kr:8443"),
            None,
            &a
        ));
        assert!(!origin_is_trusted(
            Some("https://voca.example.kr:9999"),
            None,
            &a
        ));
    }

    #[test]
    fn the_throttle_stops_repeated_attempts() {
        let t = LoginThrottle::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for i in 0..MAX_ATTEMPTS as i64 {
            assert!(t.check("a@b.kr", Some(ip), 1000 + i).is_ok(), "시도 {i}");
        }
        assert!(t.check("a@b.kr", Some(ip), 1008).is_err());
    }

    #[test]
    fn the_throttle_does_not_block_other_accounts() {
        // 아는 계정을 잠가서 DoS를 하면 안 된다.
        let t = LoginThrottle::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for i in 0..MAX_ATTEMPTS as i64 {
            t.check("a@b.kr", Some(ip), 1000 + i).unwrap();
        }
        assert!(t.check("b@b.kr", Some(ip), 1005).is_ok());
    }

    #[test]
    fn the_throttle_does_not_block_other_ips() {
        // 한쪽만 잠그면 다른 쪽으로 우회된다. (이메일, IP) 쌍으로 센다.
        let t = LoginThrottle::new();
        let attacker: IpAddr = "6.6.6.6".parse().unwrap();
        let victim: IpAddr = "7.7.7.7".parse().unwrap();
        for i in 0..MAX_ATTEMPTS as i64 {
            t.check("a@b.kr", Some(attacker), 1000 + i).unwrap();
        }
        assert!(t.check("a@b.kr", Some(victim), 1005).is_ok());
    }

    #[test]
    fn the_throttle_window_expires() {
        let t = LoginThrottle::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for i in 0..MAX_ATTEMPTS as i64 {
            t.check("a@b.kr", Some(ip), 1000 + i).unwrap();
        }
        assert!(t.check("a@b.kr", Some(ip), 1008).is_err());
        // 창이 지나면 다시 된다.
        assert!(
            t.check("a@b.kr", Some(ip), 1000 + WINDOW_SECONDS + 1)
                .is_ok()
        );
    }

    #[test]
    fn a_success_clears_the_counter() {
        let t = LoginThrottle::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for i in 0..MAX_ATTEMPTS as i64 {
            t.check("a@b.kr", Some(ip), 1000 + i).unwrap();
        }
        assert!(t.check("a@b.kr", Some(ip), 1008).is_err());

        t.record_success("a@b.kr", Some(ip));
        assert!(t.check("a@b.kr", Some(ip), 1009).is_ok());
    }
}

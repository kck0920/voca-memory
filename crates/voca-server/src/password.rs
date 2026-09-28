//! 비밀번호 해시.
//!
//! **argon2id** 다. OWASP가 현재 권하는 방식이고, GPU 공격에 강하다.
//! (bcrypt는 GPU 사당값이 떨어졌고, scrypt는 구현이 미묘하게 갈린다.)
//!
//! 파라미터를 하드코딩하지 않는다 — `Params::default()`가 argon2id의 권장값이다.
//! 나중에 세게 올리려면 `Params` 를 바꾸고, 기존 해시는 저장된 문자열에 파라미터가
//! 들어 있으므로 **투명하게 재검증**된다. 그게 PHC 문자열 형식의 요점이다.

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};

/// 비밀번호 최소 길이. **글자 수**로 센다.
///
/// 8자를 넘어가면 대다수 사용자에게 기억 부담만 늘고 실질 강도는 크게 안 오른다.
/// 바이트로 세면 안 된다 — 한국어 8바이트는 2.6글자에 불과해, 한국어 사용자에게
/// 실질 최소 길이가 3글자밖에 안 된다.
pub const MIN_PASSWORD_LEN: usize = 8;

/// 최대 길이. **바이트 수**로 센다.
///
/// argon2가 입력을 그대로 읽으므로 상한이 DoS 방어선이 된다. 이건 바이트가 맞다 —
/// 비용이 글자 수가 아니라 처리하는 바이트에 비례한다.
pub const MAX_PASSWORD_BYTES: usize = 256;

/// 비밀번호를 해시해 PHC 문자열로 돌려준다.
pub fn hash(password: &str) -> Result<String, HashError> {
    check_length(password)?;
    // argon2 0.6 는 salt 를 스스로 뽑는다. 직접 만들면 매번 같은 salt 를 넣는
    // 실수를 할 수 있고, 그건 rainbow table 로 바로 이어진다.
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| HashError)
}

/// PHC 문자열과 비밀번호를 비교한다.
///
/// **해시 문자열이 손상됐으면 `false` 다** — 예외를 던지지 않는다. 저장된 값이
/// 이상하면 그 계정은 로그인할 수 없어야 하고, 서버가 죽어서는 안 된다.
pub fn verify(stored: &str, password: &str) -> bool {
    if password.len() > MAX_PASSWORD_BYTES {
        return false;
    }
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    // 저장된 PHC 문자열에 파라미터가 들어 있으므로 여기서 파라미터를 다시 정할 필요는
    // 없다. 나중에 세게 올려도 **기존 해시가 그대로 검증된다.**
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// 시간을 벌어서 "계정이 없음"과 "비밀번호가 틀림"을 구별하지 못하게 한다.
///
/// 이메일이 없으면 실제로는 아무 검증도 하지 않는다. 그 즉시 401을 돌려주면 응답
/// 시간만으로 "이 이메일은 가입돼 있다"를 알아낼 수 있다. 그래서 없는 계정은
/// **더미 해시 검증**을 한 뒤 같은 시간을 쓴다.
pub fn burn_time() {
    // **실제 검증과 같은 파라미터**로 더미 해시를 계산한다. 결과는 버린다.
    // 시간을 맞추려면 검증 경로와 비용이 같아야 한다.
    let _ = Argon2::default().hash_password(b"timing-equalizer-not-a-real-password");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashError;

fn check_length(password: &str) -> Result<(), HashError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(HashError);
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(HashError);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hashed_password_verifies() {
        let h = hash("부지런한-비밀번호").unwrap();
        assert!(verify(&h, "부지런한-비밀번호"));
    }

    #[test]
    fn the_wrong_password_fails() {
        let h = hash("부지런한-비밀번호").unwrap();
        assert!(!verify(&h, "다른 비밀번호"));
    }

    #[test]
    fn the_hash_is_not_the_password() {
        let h = hash("부지런한-비밀번호").unwrap();
        assert!(!h.contains("부지런한"));
    }

    #[test]
    fn the_same_password_hashes_differently_each_time() {
        // salt 가 없으면 두 해시가 같아지고, rainbow table 에 바로 걸린다.
        let a = hash("똑같은-비밀번호").unwrap();
        let b = hash("똑같은-비밀번호").unwrap();
        assert_ne!(a, b, "salt 가 동작하지 않는다");
        assert!(verify(&a, "똑같은-비밀번호") && verify(&b, "똑같은-비밀번호"));
    }

    #[test]
    fn the_hash_names_argon2id() {
        assert!(hash("x-pass-word").unwrap().starts_with("$argon2id$"));
    }

    #[test]
    fn a_short_password_is_refused() {
        // 7자 — 최소 8자 미만.
        assert_eq!(hash("짧은비번"), Err(HashError));
    }

    #[test]
    fn the_minimum_counts_characters_not_bytes() {
        // 한국어 8글자는 24바이트다. 바이트로 셌으면 이걸 "짧다"고 막아버리고,
        // 한국어 사용자에게 실질 최소 길이가 3글자가 된다.
        let eight_korean = "가나다라마바사아";
        assert_eq!(eight_korean.chars().count(), MIN_PASSWORD_LEN);
        assert!(
            eight_korean.len() > MIN_PASSWORD_LEN,
            "바이트 수가 글자 수보다 크다"
        );
        assert!(hash(eight_korean).is_ok(), "한국어 8글자를 거절했다");
    }

    #[test]
    fn an_absurdly_long_password_is_refused() {
        let long = "가".repeat(MAX_PASSWORD_BYTES);
        assert_eq!(hash(&long), Err(HashError));
    }

    #[test]
    fn a_corrupt_stored_hash_verifies_to_false_instead_of_panicking() {
        // 저장값이 이상하면 그 계정은 로그인할 수 없어야 하고, 서버는 살아 있어야 한다.
        assert!(!verify("아무거나", "아무거나"));
        assert!(!verify("", "비밀번호"));
        assert!(!verify("$argon2id$", "비밀번호"));
    }

    #[test]
    fn an_absurd_candidate_fails_fast() {
        // 검증 전에 길이만 보고 끊는다. argon2가 1GB 입력을 그대로 먹으면 DoS다.
        let h = hash("부지런한-비밀번호").unwrap();
        let huge = "가".repeat(MAX_PASSWORD_BYTES + 1);
        assert!(!verify(&h, &huge));
    }

    #[test]
    fn burning_time_does_not_panic() {
        burn_time();
    }
}

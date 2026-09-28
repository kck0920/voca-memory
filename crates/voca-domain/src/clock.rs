use time::{Date, OffsetDateTime, UtcOffset};

/// 실제로 존재하는 최대 UTC 오프셋. UTC−12 ~ UTC+14.
pub const MAX_ABS_UTC_OFFSET_SECONDS: i32 = 14 * 3600;

/// 클라이언트가 보낸 UTC 오프셋을 수용 범위로 잘라낸다.
///
/// 사용자는 시계를 조작할 수 있다. 스트릭을 늘리려고 absurd한 오프셋을 보내는 것을
/// 막되, 정상적인 값이 걸러지지 않도록 ±14시간 안팎은 그대로 통과시킨다.
pub fn clamp_utc_offset_seconds(seconds: i32) -> i32 {
    seconds.clamp(-MAX_ABS_UTC_OFFSET_SECONDS, MAX_ABS_UTC_OFFSET_SECONDS)
}

/// 주어진 시각을 로컬 날짜로 환산한다.
///
/// `utc_offset_seconds`는 **세션 동안 고정된 값**이어야 한다. 접속 시점에 한 번
/// 정하고 세션이 끝날 때까지 다시 쓰지 않는다 — 브라우저를 자정 넘겨 열어놓은
/// 채로 두어도 스트릭이 뒤집히지 않게 하기 위해서다.
pub fn local_date(at: OffsetDateTime, utc_offset_seconds: i32) -> Option<Date> {
    UtcOffset::from_whole_seconds(clamp_utc_offset_seconds(utc_offset_seconds))
        .ok()
        .map(|offset| at.to_offset(offset).date())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn converts_to_local_date() {
        // KST = UTC+9. 자정 직전 UTC는 그날 하루 지났다고 봐야 한다.
        let just_before_utc_midnight = datetime!(2026-03-01 23:30:00 UTC);
        assert_eq!(
            local_date(just_before_utc_midnight, 9 * 3600),
            Some(date!(2026 - 03 - 02))
        );
    }

    #[test]
    fn same_utc_instant_is_different_day_in_each_offset() {
        let at = datetime!(2026-03-02 02:00:00 UTC);
        assert_eq!(local_date(at, 0), Some(date!(2026 - 03 - 02)));
        assert_eq!(local_date(at, 9 * 3600), Some(date!(2026 - 03 - 02)));
        assert_eq!(local_date(at, -5 * 3600), Some(date!(2026 - 03 - 01)));
    }

    #[test]
    fn clamps_absurd_offsets_into_range() {
        assert_eq!(
            clamp_utc_offset_seconds(100 * 3600),
            MAX_ABS_UTC_OFFSET_SECONDS
        );
        assert_eq!(
            clamp_utc_offset_seconds(-100 * 3600),
            -MAX_ABS_UTC_OFFSET_SECONDS
        );
        assert_eq!(clamp_utc_offset_seconds(9 * 3600), 9 * 3600);
    }

    #[test]
    fn accepts_every_clamped_offset_as_a_real_offset() {
        for raw in [-100 * 3600, -14 * 3600, 0, 9 * 3600, 14 * 3600, 100 * 3600] {
            let at = datetime!(2026-03-02 02:00:00 UTC);
            assert!(
                local_date(at, raw).is_some(),
                "offset {raw} should map to a real date"
            );
        }
    }
}

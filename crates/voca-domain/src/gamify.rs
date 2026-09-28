use serde::{Deserialize, Serialize};

use crate::Rating;

/// Rating 1건이 주는 XP.
///
/// **이 표는 고정한다.** `xp_events`가 쌓인 뒤에 바꾸면 과거 데이터 해석이 갈린다.
///
/// `Again`이 0인 것은 의도적이다. 실패에 보상을 주면 학습자가 `Again`을 누르는 걸
/// 꺼리게 되고, 그러면 FSRS가 가장 필요로 하는 신호 자체가 오염된다.
/// 보상을 줘야 하는 것은 복습 행위이지 정답이 아니다.
pub const XP_PER_RATING: [u32; 4] = [
    0, // Again
    1, // Hard
    2, // Good
    4, // Easy
];

pub fn xp_for(rating: Rating) -> u32 {
    XP_PER_RATING[rating.as_u8() as usize - 1]
}

/// 목표 기억 유지율 프리셋.
///
/// 사용자에게 `0.90` 같은 숫자를 보여줄 수 없으므로 연속값이 아니라 세 칸만 노출한다.
/// 각 값의 실제 일일 리뷰 부하는 [`docs/design.md`](../../docs/design.md)의 Phase 1
/// 시뮬레이션에서 측정해 확정했다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPreset {
    /// 자주 복습하고 기억을 오래 붙잡는다. 예상 일일 부하가 가장 많다.
    Diligent,
    /// 기준값.
    #[default]
    Balanced,
    /// 적게 복습하고 부하를 아낀다. 까먹을 위험이 가장 높다.
    Frugal,
}

impl RetentionPreset {
    pub const ALL: [RetentionPreset; 3] = [
        RetentionPreset::Diligent,
        RetentionPreset::Balanced,
        RetentionPreset::Frugal,
    ];

    /// FSRS의 `desired_retention`. 기억이 남아 있을 확률.
    pub fn desired_retention(self) -> f32 {
        match self {
            RetentionPreset::Diligent => 0.95,
            RetentionPreset::Balanced => 0.90,
            RetentionPreset::Frugal => 0.85,
        }
    }
}

/// 레벨 곡선. `level = floor(sqrt(total_xp / 200)) + 1`
///
/// 기초 구간은 빠르고 시간이 갈수록 평평해진다. 상한을 두지 않아 비교 가능성을
/// 잃지 않으면서 오래 플레이할 수 있다.
pub fn level_for(total_xp: u64) -> u32 {
    let level = (total_xp as f64 / 200.0).sqrt().floor() + 1.0;
    if level > f64::from(u32::MAX) {
        u32::MAX
    } else {
        level as u32
    }
}

/// 다음 레벨까지 필요한 누적 XP.
pub fn xp_for_next_level(level: u32) -> u64 {
    200 * u64::from(level) * u64::from(level)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelProgress {
    pub level: u32,
    /// 현재 레벨에서 이미 쌓은 XP.
    pub xp_into_level: u32,
    /// 현재 레벨에서 다음 레벨까지 필요한 총 XP.
    pub xp_span: u32,
}

/// 누적 XP에서 레벨 진행도를 계산한다.
pub fn xp_progress(total_xp: u64) -> LevelProgress {
    let level = level_for(total_xp);
    let level_floor = 200 * u64::from(level - 1) * u64::from(level - 1);
    let level_ceiling = xp_for_next_level(level);

    let into = total_xp.saturating_sub(level_floor);
    let span = level_ceiling.saturating_sub(level_floor);

    LevelProgress {
        level,
        xp_into_level: u32::try_from(into).unwrap_or(u32::MAX),
        xp_span: u32::try_from(span).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn again_is_worth_nothing() {
        assert_eq!(xp_for(Rating::Again), 0);
    }

    #[test]
    fn xp_rises_monotonically_with_rating() {
        let xs: Vec<u32> = Rating::ALL.iter().map(|r| xp_for(*r)).collect();
        assert_eq!(xs, vec![0, 1, 2, 4]);
        for pair in xs.windows(2) {
            assert!(pair[0] < pair[1], "XP must strictly increase: {xs:?}");
        }
    }

    #[test]
    fn rating_persists_as_one_through_four() {
        for rating in Rating::ALL {
            assert_eq!(Rating::from_u8(rating.as_u8()), Some(rating));
        }
        assert_eq!(Rating::from_u8(0), None);
        assert_eq!(Rating::from_u8(5), None);
    }

    #[test]
    fn level_starts_at_one_with_no_xp() {
        assert_eq!(level_for(0), 1);
        assert_eq!(xp_progress(0).level, 1);
        assert_eq!(xp_progress(0).xp_into_level, 0);
    }

    #[test]
    fn level_boundaries_land_on_squares() {
        // level n은 200 * (n-1)^2 에서 오른다.
        assert_eq!(level_for(199), 1);
        assert_eq!(level_for(200), 2);
        assert_eq!(level_for(799), 2);
        assert_eq!(level_for(800), 3);
        assert_eq!(level_for(1_800), 4);
    }

    #[test]
    fn level_never_goes_backwards() {
        let mut previous = level_for(0);
        for xp in (0..50_000).step_by(97) {
            let level = level_for(xp);
            assert!(level >= previous, "level dropped at xp={xp}");
            previous = level;
        }
    }

    #[test]
    fn progress_within_a_level_stays_inside_its_span() {
        for xp in [0, 199, 200, 799, 800, 1_800, 10_000, 1_000_000] {
            let p = xp_progress(xp);
            assert!(
                p.xp_into_level < p.xp_span,
                "xp={xp} produced {p:?} with no room left"
            );
            assert!(p.xp_span > 0);
        }
    }

    #[test]
    fn presets_are_ordered_by_retention() {
        let diligent = RetentionPreset::Diligent.desired_retention();
        let balanced = RetentionPreset::Balanced.desired_retention();
        let frugal = RetentionPreset::Frugal.desired_retention();
        assert!(diligent > balanced, "{diligent} !> {balanced}");
        assert!(balanced > frugal, "{balanced} !> {frugal}");
        for preset in RetentionPreset::ALL {
            let r = preset.desired_retention();
            assert!(
                (0.0..1.0).contains(&r),
                "{preset:?} retention {r} out of range"
            );
        }
    }

    #[test]
    fn balanced_is_the_default() {
        assert_eq!(RetentionPreset::default(), RetentionPreset::Balanced);
    }
}

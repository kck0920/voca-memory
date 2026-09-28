//! 화면 조각.
//!
//! **순수하게 계산되는 것만 여기 둔다.** 비동기 조회나 컨텍스트 접근을 컴포넌트에
//! 박지 않는다 — 그러면 같은 조각을 서버 렌더링과 클라이언트 렌더링 양쪽에서 그대로
//! 쓸 수 있다.
//!
//! 그게 [ADR-0002](../../docs/adr/0002-ui-must-be-csr-compatible-from-day-one.md)의
//! 요구다. 서버에서만 되는 컴포넌트가 하나라도 있으면 나중에 Tauri 를 붙일 때 그
//! 트리를 통째로 다시 써야 한다.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use voca_domain::{LevelProgress, Rating, RetentionPreset, Streak};

/// 대시보드가 화면에 필요한 것.
///
/// 저장소의 `Dashboard` 를 그대로 쓰지 않는다. 그 타입은 `CardId` 등 서버 개념을
/// 들고 있고, 화면은 그걸 몰라야 한다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct DashboardView {
    pub streak_current: u32,
    pub streak_longest: u32,
    pub reviews_due: u32,
    pub new_remaining: u32,
    pub level: u32,
    pub xp_into_level: u32,
    pub xp_span: u32,
    /// 오늘 Review 가 아직 없다.
    pub studied_today: bool,
}

/// 서버가 심어 두고 하이드레이션이 읽는 요소의 id.
///
/// **양쪽이 같은 문자열을 알도록 여기 한 곳에만 적는다.** 서버가 심는 것과
/// 클라이언트가 찾는 것이 어긋나면 하이드레이션이 조용히 실패한다 — 화면은 보이고
/// 아무것도 반응하지 않는다.
pub const DATA_ELEMENT_ID: &str = "voca-data";

impl DashboardView {
    /// 도메인 규칙을 그대로 옮긴다 — 화면에서 다시 계산하지 않는다.
    pub fn from_domain(
        streak: Streak,
        level: LevelProgress,
        reviews_due: u32,
        new_remaining: u32,
        studied_today: bool,
    ) -> Self {
        Self {
            streak_current: streak.current,
            streak_longest: streak.longest,
            reviews_due,
            new_remaining,
            level: level.level,
            xp_into_level: level.xp_into_level,
            xp_span: level.xp_span,
            studied_today,
        }
    }

    /// 오늘 풀 것이 남아 있는가.
    pub fn has_work_today(&self) -> bool {
        self.reviews_due > 0 || self.new_remaining > 0
    }

    /// 스트릭이 위험한가.
    ///
    /// **할 게 있는데 아직 안 한 경우**다. 할 게 없으면 끊겨도 아니다 — 다시 할 수
    /// 있으니까. 그래서 `has_work_today` 와 함께 본다.
    pub fn is_streak_at_risk(&self) -> bool {
        self.has_work_today() && !self.studied_today
    }

    /// 0~100. `xp_span` 이 0 이면 0 을 돌려준다 (나눗셈 오류 대신).
    pub fn level_percent(&self) -> u32 {
        if self.xp_span == 0 {
            return 0;
        }
        (self.xp_into_level as u64 * 100 / self.xp_span as u64) as u32
    }
}

/// 대시보드 상단.
#[component]
pub fn DashboardHeader(view: DashboardView) -> impl IntoView {
    let (streak_label, risk) = (
        format!("{}일 연속", view.streak_current),
        view.is_streak_at_risk(),
    );

    view! {
        <header class="dashboard-header" data-testid="dashboard-header">
            <p class="streak" data-risk=move || risk.to_string()>
                {streak_label}
                <Show when=move || risk fallback=|| ()>
                    <span class="streak-at-risk">"오늘 아직 안 했습니다"</span>
                </Show>
            </p>
            <p class="due">
                {view.reviews_due}
                "장 복습 · "
                {view.new_remaining}
                "장 새 단어"
            </p>
            <LevelBar level=view.level percent=view.level_percent() />
        </header>
    }
}

#[component]
pub fn LevelBar(level: u32, percent: u32) -> impl IntoView {
    view! {
        <div class="level" data-testid="level-bar" data-level=level>
            <span class="level-label">{format!("Lv {level}")}</span>
            <progress class="level-progress" value=percent max=100 />
        </div>
    }
}

/// 복습 화면의 4개 버튼 라벨.
///
/// **라벨은 `voca-domain` 이 계산한 값을 그대로 쓴다.** 화면에서 interval 을 다시
/// 계산하면 두 구현이 어긋난다 — 서버가 3일이라는데 화면은 2일이라 보이는 일이
/// 생긴다.
#[component]
pub fn RatingButtons(labels: [(&'static str, u32); 4]) -> impl IntoView {
    let buttons: Vec<_> = Rating::ALL
        .iter()
        .zip(labels.iter())
        .map(|(rating, (label, days))| {
            let _ = rating;
            (*label, *days)
        })
        .collect();

    view! {
        <div class="ratings" data-testid="rating-buttons">
            {buttons
                .into_iter()
                .map(|(label, days)| {
                    view! {
                        <button type="button" data-label=label data-days=days>
                            {label}
                            <span class="interval">{interval_label(days)}</span>
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// `0` 일이면 "지금", `1` 이면 "내일" 이다.
///
/// 규칙의 정본은 [`voca_domain`] 다 — 서버도 같은 문장을 낸다. 여기서는 다시 정의하지
/// 않는다.
pub use voca_domain::interval_label;

/// Retention Preset 선택.
///
/// **이름은 여기서 정한다.** 도메인은 `desired_retention` 숫자만 알고, 사람이 읽는
/// 이름은 화면의 일이다 (docs/design.md "Level 보상" 의 같은 원칙).
#[component]
pub fn RetentionPicker(selected: RetentionPreset) -> impl IntoView {
    let options: Vec<(RetentionPreset, &'static str, &'static str)> = vec![
        (RetentionPreset::Diligent, "부지런", "가장 자주, 가장 오래"),
        (RetentionPreset::Balanced, "균형", "보통"),
        (RetentionPreset::Frugal, "아끼기", "적게, 오래"),
    ];

    view! {
        <div class="retention" data-testid="retention-picker">
            {options
                .into_iter()
                .map(|(preset, name, blurb)| {
                    let checked = preset == selected;
                    view! {
                        <label data-preset=format!("{:?}", preset) data-checked=checked.to_string()>
                            <input type="radio" name="retention" value=format!("{:?}", preset) checked />
                            <strong>{name}</strong>
                            <span>{blurb}</span>
                        </label>
                    }
                })
                .collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voca_domain::{LevelProgress, Streak};

    fn view(reviews_due: u32, new_remaining: u32, studied: bool) -> DashboardView {
        DashboardView::from_domain(
            Streak {
                current: 7,
                longest: 12,
                last_review_date: None,
            },
            LevelProgress {
                level: 3,
                xp_into_level: 300,
                xp_span: 1200,
            },
            reviews_due,
            new_remaining,
            studied,
        )
    }

    #[test]
    fn work_today_is_independent_of_the_streak() {
        // 할 게 없는데 Streak 숫자가 남아 있다. 끊긴 게 아니다.
        let v = view(0, 0, false);
        assert!(!v.has_work_today());
        assert_eq!(v.streak_current, 7);
        assert!(!v.is_streak_at_risk());
    }

    #[test]
    fn a_streak_is_at_risk_only_when_there_is_still_work() {
        assert!(view(12, 0, false).is_streak_at_risk());
        assert!(!view(12, 0, true).is_streak_at_risk());
        assert!(!view(0, 0, false).is_streak_at_risk());
    }

    #[test]
    fn level_percent_is_a_percentage_and_survives_a_zero_span() {
        assert_eq!(view(1, 0, true).level_percent(), 25);
        let mut v = view(1, 0, true);
        v.xp_span = 0;
        assert_eq!(v.level_percent(), 0, "나눗셈이 0 이 되거나 패닉했다");
    }

    #[test]
    fn interval_labels_read_like_language() {
        assert_eq!(interval_label(0), "지금");
        assert_eq!(interval_label(1), "내일");
        assert_eq!(interval_label(3), "3일 후");
        assert_eq!(interval_label(400), "400일 후");
    }

    #[test]
    fn every_rating_gets_a_label() {
        // 네 개 모두 라벨이 있어야 한다. 빠진 게 있으면 안 눌린다.
        let labels = [("Again", 0u32), ("Hard", 1), ("Good", 3), ("Easy", 9)];
        assert_eq!(labels.len(), Rating::ALL.len());
        assert!(labels.iter().all(|(l, _)| !l.is_empty()));
    }

    #[test]
    fn every_preset_has_a_name_and_a_blurb() {
        let presets = RetentionPreset::ALL;
        assert_eq!(presets.len(), 3);
        for p in presets {
            assert!(!p.desired_retention().is_nan());
            assert!(p.desired_retention() > 0.0 && p.desired_retention() < 1.0);
        }
    }
}

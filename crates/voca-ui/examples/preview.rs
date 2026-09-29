//! 화면 상태를 한 장의 HTML 로 모아서 미리 본다.
//!
//! **목적은 "이 상태가 실제로 어떻게 그려지는가" 를 눈으로 확인하는 것이다.**
//! 서버를 띄우고 계정을 만들고 카드를 넣어야 닿는 상태들(0장, 스트릭 위험,
//! 채워진 게이지, 아직 마운트되지 않은 컴포넌트)을 여기서는 한 번에 본다.
//!
//! 서버를 쓰지 않는 이유는 **미리보기에 실제 상태가 필요 없기 때문이다.** 데이터는
//! 전부 아래에 박혀 있다. 그래서 여기서 맞게 그려졌다고 실제 화면이 맞는 건 아니다 —
//! 실데이터는 `./scripts/smoke.sh` 나 브라우저로 확인한다.
//!
//! ```text
//! cargo run -p voca-ui --features ssr --example preview
//! # 산출물: target/ui-preview.html
//! ```
//!
//! 인자로 경로를 주면 그 경로에 쓴다. **저장소 루트에 쓰지 마라** — 미리보기
//! 산출물이 커밋에 딸려 들어간다.

use std::fs;
use std::path::PathBuf;

use leptos::prelude::*;
use voca_domain::{LevelProgress, RetentionPreset, Streak};
use voca_ui::view::{DashboardHeader, DashboardView, RatingButtons, RetentionPicker};

/// 로그아웃 화면이 그리는 값.
///
/// `page.rs` 의 `logged_out()` 과 **같아야 한다.** 다르면 여기서 미리본 0 상태와
/// 실제로 로그아웃했을 때의 화면이 달라져서, 원인을 찾을 때마다 두 곳을 같이 본다.
fn logged_out() -> DashboardView {
    DashboardView {
        streak_current: 0,
        streak_longest: 0,
        reviews_due: 0,
        new_remaining: 0,
        level: 1,
        xp_into_level: 0,
        xp_span: 200,
        studied_today: false,
    }
}

fn main() {
    // 본문에 쓰는 스타일은 페이지가 페이지에 넣는 것과 **같은 파일**이어야 한다.
    // 다른 파일을 쓰면 미리보기가 실제 화면과 달라진다 — 확인할 가치가 없어진다.
    let css = include_str!("../assets/retro.css");

    let calm = DashboardView::from_domain(
        Streak {
            current: 7,
            longest: 12,
            last_review_date: None,
        },
        LevelProgress {
            level: 3,
            xp_into_level: 540,
            xp_span: 1200,
        },
        5,
        10,
        true,
    );

    let risk = DashboardView::from_domain(
        Streak {
            current: 15,
            longest: 20,
            last_review_date: None,
        },
        LevelProgress {
            level: 4,
            xp_into_level: 150,
            xp_span: 1400,
        },
        12,
        3,
        false,
    );

    let rating_labels = [("Again", 0u32), ("Hard", 1), ("Good", 3), ("Easy", 9)];

    let body = Owner::new().with(|| {
        let view = view! {
            <main class="app">
                <h1 class="brand">"Voca Memory"</h1>
                <p class="brand-sub">"영어 어휘장 · 복습 터미널"</p>

                <h2 class="preview-label">"1. 로그아웃 — 할 것이 없음"</h2>
                <DashboardHeader view=logged_out() />
                <p class="empty-note">"단어를 추가하면 여기에서 복습을 시작합니다."</p>

                <h2 class="preview-label">"2. 진행 중 — 7일 연속, 오늘 완료, 45% XP"</h2>
                <DashboardHeader view=calm />

                <h2 class="preview-label">"3. 스트릭 위험 — 15일 연속, 오늘 미완료"</h2>
                <DashboardHeader view=risk />

                <h2 class="preview-label">"4. 복습 평가 버튼 — 아직 App 에 마운트되지 않음"</h2>
                <RatingButtons labels=rating_labels />

                <h2 class="preview-label">"5. 프리셋 선택기 — 아직 App 에 마운트되지 않음"</h2>
                <RetentionPicker selected=RetentionPreset::Balanced />
            </main>
        };
        view.into_view().to_html()
    });

    let page = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Voca Memory UI 미리보기</title>
<style>{css}</style>
</head>
<body>
{body}
</body>
</html>
"#
    );

    let out: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/ui-preview.html"));

    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).expect("출력 디렉터리를 못 만들었다");
    }
    fs::write(&out, page).expect("미리보기를 못 썼다");
    println!("미리보기: {}", out.display());
}

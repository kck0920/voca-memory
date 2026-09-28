//! Voca Memory 화면.
//!
//! [ADR-0002](../../docs/adr/0002-ui-must-be-csr-compatible-from-day-one.md): 이
//! 트리는 **서버 렌더링과 클라이언트 렌더링 양쪽에서 그대로** 돌아간다. 서버에서만
//! 되는 컴포넌트는 여기 없다.
//!
//! 두 진입점이 같은 [`App`] 을 쓴다는 점이 그 검증이다. Tauri 는 로컬 서버를 띄울
//! 수 없어서 데스크톱·모바일 빌드는 `mount` 로만 돈다 — 그때 이 트리가 그대로 쓸 수
//! 있어야 한다.

pub mod view;

use leptos::prelude::*;
use view::{DashboardHeader, DashboardView};

/// 앱 전체.
///
/// 데이터를 받는 곳은 두 가지뿐이고, 둘 다 **서버가 계산해서 넘겨준다**:
/// 화면이 직접 조회하지 않는다. 조회하면 서버 렌더링과 클라이언트 렌더링이 서로
/// 다른 값을 그릴 수 있다.
#[component]
pub fn App(initial: DashboardView) -> impl IntoView {
    view! {
        <main class="app">
            <h1>"Voca Memory"</h1>
            <DashboardHeader view=initial />
            <p class="empty-note">
                "단어를 추가하면 여기에서 복습을 시작합니다."
            </p>
        </main>
    }
}

/// SSR 용 진입점.
///
/// Leptos 0.8 에는 `render_to_string` 이 없다. 컴포넌트가 만든 뷰가 `RenderHtml` 을
/// 구현하므로 `.to_html()` 로 문자열이 된다. 소유자(`Owner`)는 직접 만들어 연다 —
/// 서버 쪽 `leptos_axum` 가 이걸 해 주지만, 여기서 직접 쓸 수 있어야
/// **UI 를 서버 없이 시험할 수 있다.**
#[cfg(feature = "ssr")]
pub fn render_app(initial: DashboardView) -> String {
    use leptos::prelude::{IntoView, Owner, RenderHtml};
    Owner::new().with(|| App(AppProps { initial }).into_view().to_html())
}

/// 서버가 이미 그려 놓은 페이지를 물린다.
///
/// SSR 로 받은 마크업을 **버리지 않고** 거기에 이벤트를 건다. 버리면 스크롤 위치와
/// 입력 중이던 것이 사라진다.
#[cfg(feature = "hydrate")]
pub fn hydrate_app(initial: DashboardView) {
    leptos::mount::hydrate_body(move || App(AppProps { initial }));
}

/// 서버 없이 처음부터 그린다.
///
/// Tauri 가 이 경로다 — 로컬 서버가 없으므로 SSR 도, 하이드레이션도 할 수 없다.
/// **같은 [`App`] 트리를 쓴다는 점이 SSR 경로와의 유일한 차이다.**
#[cfg(feature = "csr")]
pub fn mount_app(initial: DashboardView) {
    leptos::mount::mount_to_body(move || App(AppProps { initial }));
}

/// 문서 전체를 서버에서 만든다.
///
/// `leptos_axum` 가 하는 일을 직접 한다. 프레임워크를 하나 더 얹는 대신 HTML 껍데기
/// 30줄을 우리가 소유한다 — 무엇이 나가는지 눈에 보이고, `wasm` 번들이 안 떠도
/// 페이지가 온전히 보인다.
#[cfg(feature = "ssr")]
pub fn render_page(initial: DashboardView, script_src: &str) -> String {
    use leptos::prelude::{IntoView, Owner, RenderHtml};

    let body = Owner::new().with(|| App(AppProps { initial }).into_view().to_html());

    // 데이터를 JSON 으로 심어 둔다. `type="application/json"` 인 script 는 실행되지
    // 않으므로 XSS 로 새지 않는다. 페이지를 동결(`Object.freeze`)하지 않는다 —
    // 그건 눈여겨볼 만한 공격이지, 여기서는 문제가 아니다.
    let payload = serde_json::to_string(&initial).unwrap_or_else(|_| "{}".to_owned());

    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Voca Memory</title>
</head>
<body>
{body}
<script type="application/json" id="{data_id}">{payload}</script>
<script type="module" src="{script_src}"></script>
</body>
</html>
"#,
        data_id = view::DATA_ELEMENT_ID,
    )
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "ssr")]
    use super::*;

    /// SSR 테스트만 쓴다. CSR 빌드에서는 이 모듈이 비어 있다.
    #[cfg(feature = "ssr")]
    fn sample() -> DashboardView {
        DashboardView::from_domain(
            voca_domain::Streak {
                current: 7,
                longest: 12,
                last_review_date: None,
            },
            voca_domain::LevelProgress {
                level: 3,
                xp_into_level: 300,
                xp_span: 1200,
            },
            12,
            3,
            false,
        )
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_page_is_a_whole_document_with_the_hydration_hook() {
        let page = render_page(sample(), "/pkg/voca-web.js");
        assert!(page.starts_with("<!DOCTYPE html>"), "{page}");
        assert!(page.contains(r#"<html lang="ko">"#), "{page}");
        assert!(page.contains("/pkg/voca-web.js"), "{page}");
        // 데이터는 실행되지 않는 script 안에 있다.
        assert!(page.contains(r#"type="application/json""#), "{page}");
        assert!(page.contains(view::DATA_ELEMENT_ID), "{page}");
        // 본문이 비어 있으면 안 된다 — 마크업만 나오는 페이지는 아무것도 못 한다.
        assert!(page.contains("Voca Memory"), "{page}");
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_embedded_payload_round_trips() {
        // 서버가 심은 JSON 을 하이드레이션이 그대로 읽는다. 형식이 어긋나면
        // 하이드레이션이 조용히 실패한다.
        let page = render_page(sample(), "/pkg/x.js");
        let start = page.find(r#"id="voca-data">"#).expect("데이터 요소가 없다")
            + r#"id="voca-data">"#.len();
        let end = page[start..].find("</script>").expect("닫는 태그가 없다") + start;
        let json = &page[start..end];

        let back: DashboardView = serde_json::from_str(json).expect("JSON 을 못 읽었다");
        assert_eq!(back, sample(), "왕복이 값을 잃었다");
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_app_renders_on_the_server() {
        let html = render_app(sample());
        assert!(html.contains("Voca Memory"), "{html}");
        assert!(html.contains("7일 연속"), "Streak 이 안 보인다: {html}");
        assert!(
            html.contains("오늘 아직 안 했습니다"),
            "위험 표시가 없다: {html}"
        );
        assert!(html.contains("Lv 3"), "레벨이 안 보인다: {html}");
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn rendering_twice_gives_the_same_markup() {
        // 결정적이어야 한다. 서버 렌더링과 하이드레이션이 다른 트리를 그리면
        // 브라우저가 앵커를 잃는다.
        assert_eq!(render_app(sample()), render_app(sample()));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn a_calm_dashboard_does_not_show_a_risk_warning() {
        let calm = DashboardView::from_domain(
            voca_domain::Streak::default(),
            voca_domain::LevelProgress {
                level: 1,
                xp_into_level: 0,
                xp_span: 200,
            },
            0,
            0,
            false,
        );
        let html = render_app(calm);
        assert!(!html.contains("오늘 아직 안 했습니다"), "{html}");
    }
}

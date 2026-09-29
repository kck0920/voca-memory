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
            <header class="app-topbar">
                <div class="brand-group">
                    <h1 class="brand">"Voca Memory"</h1>
                    <p class="brand-sub">"영어 어휘장 · 복습 터미널"</p>
                </div>
                <div id="auth-status" class="auth-status"></div>
            </header>

            <DashboardHeader view=initial />

            // 비로그인 상태: 로그인 / 회원가입 폼
            <section id="auth-section" class="panel auth-panel">
                <div class="auth-tabs">
                    <button type="button" class="tab-btn active" id="tab-login-btn">"로그인"</button>
                    <button type="button" class="tab-btn" id="tab-register-btn">"회원가입"</button>
                </div>
                <form id="login-form" class="auth-form">
                    <div class="form-row">
                        <label for="login-email">"이메일"</label>
                        <input type="email" id="login-email" required=true placeholder="user@example.com" />
                    </div>
                    <div class="form-row">
                        <label for="login-password">"비밀번호"</label>
                        <input type="password" id="login-password" required=true placeholder="8자 이상" />
                    </div>
                    <button type="submit" class="submit-btn">"로그인"</button>
                </form>
                <form id="register-form" class="auth-form hidden">
                    <div class="form-row">
                        <label for="reg-email">"이메일"</label>
                        <input type="email" id="reg-email" required=true placeholder="user@example.com" />
                    </div>
                    <div class="form-row">
                        <label for="reg-name">"닉네임"</label>
                        <input type="text" id="reg-name" required=true placeholder="학습자" />
                    </div>
                    <div class="form-row">
                        <label for="reg-password">"비밀번호"</label>
                        <input type="password" id="reg-password" required=true placeholder="8자 이상" />
                    </div>
                    <button type="submit" class="submit-btn">"회원가입하고 시작하기"</button>
                </form>
                <div id="auth-msg" class="system-msg"></div>
            </section>

            // 로그인 사용자 패널: 복습 / 단어 검색·추가 / 설정
            <section id="study-panel" class="panel study-panel hidden">
                <nav class="action-nav">
                    <button type="button" class="action-tab active" data-tab="study">"1. 복습하기"</button>
                    <button type="button" class="action-tab" data-tab="dict">"2. 단어 검색 및 추가"</button>
                    <button type="button" class="action-tab" data-tab="settings">"3. 학습 설정"</button>
                </nav>

                // 1. 복습하기 탭
                <div id="tab-study-content" class="tab-content active">
                    <div id="study-container">
                        <p class="empty-note">
                            "단어를 추가하면 여기에서 복습을 시작합니다."
                        </p>
                    </div>
                </div>

                // 2. 단어 검색 및 추가 탭
                <div id="tab-dict-content" class="tab-content hidden">
                    <form id="dict-search-form" class="search-bar">
                        <input type="text" id="dict-search-input" placeholder="영단어 검색 (예: run, keep, make, take...)" />
                        <button type="submit" class="action-btn-primary">"사전 검색"</button>
                    </form>
                    <div id="dict-results"></div>

                    <h3 class="sub-panel-title">"또는 나만의 뜻 직접 등록하기"</h3>
                    <form id="custom-sense-form" class="auth-form">
                        <div class="form-row">
                            <label for="custom-lemma">"단어 (표기형)"</label>
                            <input type="text" id="custom-lemma" required=true placeholder="run" />
                        </div>
                        <div class="form-row">
                            <label for="custom-pos">"품사 (선택)"</label>
                            <input type="text" id="custom-pos" placeholder="verb, noun 등" />
                        </div>
                        <div class="form-row">
                            <label for="custom-def">"한글 뜻 (필수)"</label>
                            <input type="text" id="custom-def" required=true placeholder="달리다, 운영하다" />
                        </div>
                        <div class="form-row">
                            <label for="custom-ex">"영어 예문 (선택)"</label>
                            <input type="text" id="custom-ex" placeholder="She runs every morning." />
                        </div>
                        <button type="submit" class="action-btn-secondary">"내 덱에 추가"</button>
                        <div id="custom-sense-msg" class="system-msg"></div>
                    </form>

                    <h3 class="sub-panel-title">"기본 단어 세트"</h3>
                    <button type="button" class="action-btn-primary" id="btn-import-seed-global">
                        "★ 기본 필수 다의어 30선 덱에 추가하기"
                    </button>
                    <div id="seed-import-feedback" class="system-msg"></div>
                </div>

                // 3. 학습 설정 탭
                <div id="tab-settings-content" class="tab-content hidden">
                    <p class="empty-note">
                        "목표 기억 유지율(Retention)을 설정합니다. 숫자가 높을수록 자주 복습합니다."
                    </p>
                    <view::RetentionPicker selected=voca_domain::RetentionPreset::Balanced />
                </div>
            </section>
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

    let payload = serde_json::to_string(&initial).unwrap_or_else(|_| "{}".to_owned());
    let css = include_str!("../assets/retro.css");
    let js = include_str!("../assets/app.js");

    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Voca Memory</title>
<style>{css}</style>
</head>
<body>
{body}
<script type="application/json" id="{data_id}">{payload}</script>
<script type="module" src="{script_src}"></script>
<script>{js}</script>
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

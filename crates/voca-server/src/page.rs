//! 페이지 렌더링.
//!
//! `/` 는 **서버에서 그린다.** 대시보드는 로그인한 사람마다 다른데, 비워 두고
//! 클라이언트가 채우면 첫 화면이 빈 대시보드로 보인다 — "내 Streak 가 왜 0 이지"가
//! 첫인상이 된다.
//!
//! 여기서 규칙을 만들지는 않는다. [`crate::study`] 가 계산한 값을 [`voca_ui`] 가
//! 그린다. 둘 다 그 사이에 있다.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use time::OffsetDateTime;
use voca_store::Store;
use voca_ui::view::DashboardView;

use crate::AppState;
use crate::routes::{ApiError, LoggedIn, require_login};

/// 하이드레이션 번들이 놓일 경로.
///
/// `cargo-leptos` 가 `site-pkg-dir` 로 복사하는 이름과 같아야 한다. 달라지면 스크립트가
/// 404 가 되고 페이지가 **조용히** interactivity 를 잃는다 — 버튼이 눌리지 않는다.
const SCRIPT_SRC: &str = "/pkg/voca-web.js";

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // 로그인 안 한 사람에게 대시보드를 그리면 안 된다. 이름도 숫자도 없다.
    let initial = match require_login(&state, &headers).await {
        Ok(LoggedIn { user, .. }) => dashboard(&state, user.user_id).await?,
        // 401 로 돌리지 않는다. **대시보드는 조용히 "로그인 안 됨"을 알아야 한다.**
        // 여기서 401 을 주면 브라우저가 그 JSON 을 페이지에 띄운다.
        Err(_) => logged_out(),
    };

    Ok(html(voca_ui::render_page(initial, SCRIPT_SRC)))
}

/// 페이지가 아니라 데이터가 필요한 화면을 위한 조회.
async fn dashboard(state: &AppState, user: voca_domain::Id) -> Result<DashboardView, ApiError> {
    let d = state
        .store()
        .dashboard(user, OffsetDateTime::now_utc())
        .await
        .map_err(ApiError::store)?;

    Ok(DashboardView {
        streak_current: d.streak.current,
        streak_longest: d.streak.longest,
        reviews_due: d.reviews_due,
        new_remaining: d.new_remaining,
        level: d.level.level,
        xp_into_level: d.level.xp_into_level,
        xp_span: d.level.xp_span,
        // `Dashboard` 가 이미 이 판정을 한다. 여기서 다시 하지 않는다 — 두 번 하면
        // 어느 한쪽만 고쳐지고 화면이 서버와 다르게 말한다.
        studied_today: d.streak.last_review_date == Some(d.local_date),
    })
}

/// 로그인 전 화면. 숫자는 전부 0 이지만 `is_streak_at_risk` 는 거짓이다 — 풀 것이
/// 없으니까 위험할 리 없다.
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

/// `text/html` 이다. `X` 가 아니다.
fn html(body: String) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

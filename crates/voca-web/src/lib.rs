//! 브라우저 진입점.
//!
//! 이 크레이트에는 **화면 로직이 없다.** 컴포넌트는 [`voca_ui`] 에 있고, 여기서는
//! 그 트리를 물리는 일만 한다. Tauri 가 이 파일을 그대로 재사용한다 — 데스크톱 앱도
//! 브라우저 엔진을 쓰고 하이드레이션만 하지 않을 뿐이다.
//!
//! 하이드레이션이 안 되면 페이지가 **멈춘 채로 보인다.** 표는 보이고 링크는 눌리지만
//! 아무것도 반응하지 않는다. 그래서 wasm 이 404 나면 그 사실이 눈에 띄어야 한다.

use wasm_bindgen::prelude::*;

/// 서버가 마크업에 심어 둔 데이터를 읽는다.
///
/// **중요:** 값을 지어내지 않는다. 못 읽으면 **하이드레이션을 하지 않는다.** 서버가
/// 이미 그려 놓은 마크업은 그 자체로 맞으므로, 거기에 0 을 얹으면 사용자는 자기 Streak
/// 를 잃은 채 화면만 살아난다. 조용히 넘어가는 것보다 멈춰 있는 편이 정직하다.
#[wasm_bindgen(start)]
pub fn hydrate() {
    console_error_panic_hook::set_once();

    match read_server_payload() {
        Ok(initial) => voca_ui::hydrate_app(initial),
        Err(e) => leptos::logging::error!(
            "voca: 서버가 넘겨준 데이터를 읽지 못했다 — {e}. \
             서버가 그린 화면은 그대로 보이지만 아무것도 반응하지 않는다."
        ),
    }
}

fn read_server_payload() -> Result<voca_ui::view::DashboardView, String> {
    use wasm_bindgen::JsCast;

    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("문서를 못 찾았다")?;

    let element = document
        .get_element_by_id(voca_ui::view::DATA_ELEMENT_ID)
        .ok_or("데이터 요소를 못 찾았다")?;

    let json = element
        .dyn_into::<web_sys::HtmlScriptElement>()
        .map_err(|_| "데이터 요소가 script 가 아니다".to_owned())?
        .text()
        .map_err(|_| "데이터 요소의 내용을 읽지 못했다".to_owned())?;

    serde_json::from_str(&json).map_err(|e| format!("JSON 을 읽지 못했다: {e}"))
}

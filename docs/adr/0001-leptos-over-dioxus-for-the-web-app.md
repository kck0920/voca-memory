# Leptos를 선택한다 (Dioxus 대신)

풀스택 Rust 웹 프레임워크로 Leptos 0.8을 쓴다. Dioxus는 아니다.

## 고려한 선택지

- **Leptos** — SSR이 실제로 동작하고, 서버 함수 경계가 `#[server]`로 명시적이며, Tauri 공식 문서에 통합 절차가 있다.
- **Dioxus** — 네이티브 타겟에 강하지만, 우리가 첫 타겟으로 정한 것은 웹이다. 네이티브 강점은 지금 당장 값하지 않고, 풀스택 성숙도에서 뒤처진다.
- **SvelteKit + TS** — 어휘 앱에 Rust를 쓰고 싶다는 동기가 있다. 이 결정의 전제를 뒤집는 선택지이므로 기재만 한다.

## 결과

Tauri를 나중에 붙일 때 Leptos CSR 빌드(`mount_to_body`)를 그대로 재사용할 수 있다. 그 전제는 [ADR-0002](./0002-ui-must-be-csr-compatible-from-day-one.md)에 있다.

## 대안의 함정

Dioxus를 고르면 "데스크톱·모바일까지"라는 목표에는 더 나을 수 있다. 하지만 웹을 먼저 만들기로 한 이상, 지금 그 이점을 살릴 수 없다. 나중에 웹 프론트를 버리고 Dioxus로 다시 쓰려면 Phase 3 이후의 모든 UI를 재작업하는 셈이다 — 그때는 이 ADR을 superseded로 표시한다.

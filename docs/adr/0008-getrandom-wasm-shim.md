# getrandom에 wasm_js shim을 건다

`voca-domain`에 `wasm32-unknown-unknown` 전용 `getrandom = { features = ["wasm_js"] }` 의존을 **target-scoped로** 추가한다.

## 왜 필요한가

`fsrs` → `rand` → `getrandom` 경로다. `getrandom` 0.4는 `wasm32-unknown-unknown`에서 `wasm_js` feature 없이는 `compile_error!`로 빌드가 깨진다. 이 검증은 Phase 1 첫 항목으로 계획했고, 실제로 그대로 실패했다.

`fsrs`에는 `rand`나 `rayon`을 끄는 feature가 없다. 둘 다 무조건 의존이다. 그래서 컴파일을 통과시키려면 shim이 유일한 선택지였다.

## 왜 shim을 받아들여도 되는가

**스케줄러는 이 shim을 런타임에 한 번도 호출하지 않는다.** `fsrs::FSRS::next_states()`는 순수 계산이고 난수를 쓰지 않는다. 검증한 사실:

- `next_states` (inference.rs) — rayon 없음, 난수 없음
- rayon은 `evaluate_with_time_series_splits`(파라미터 학습)와 `simulate()`에만 있다. 둘 다 네이티브 전용 작업이다
- 브라우저에 실리는 것은 `voca-domain`의 `Scheduler`뿐이고, 그것은 `next_states`만 호출한다

즉 shim은 **컴파일 타임에만 존재하는 세금**이다. Wasm 번들에 들어가는 바이트는 있되, 실행 경로에는 없다.

`target.'cfg(target_arch = "wasm32")'`로 걸어 네이티브 빌드에는 영향을 주지 않는다.

## 기각한 대안

- **`fsrs`를 쓰지 않고 FSRS-6 수식을 직접 구현한다.** 알고리즘 구현이 하나뿐이라 정합성 위험이 없어 보이는 대가로, 검증된 알고리즘을 다시 쓰는 것 자체가 가장 큰 리스크가 된다. 이 프로젝트는 처음부터 `fsrs`를 재작업하지 않기로 했다.
- **파라미터 학습까지 브라우저에서 돌린다.** rayon이 필요하므로 불가능하다. 애초에 그럴 필요가 없다 — 파라미터 학습은 네이티브 배치 잡의 일이다.

## 결과

이 결정은 "브라우저에서도 도메인이 돌아간다"는 전제를 지켜준다. 대신 Wasm 번들에 쓰이지 않는 JS glue가 들어간다. 번들 크기를 측정해 비용이 크면 그때 재검토한다 — 임계값은 50KB.

## 되돌리는 조건

- 브라우저 스케줄러가 필요 없어지면 (예: 오프라인 복습을 포기) shim도 필요 없어진다
- `fsrs`가 `no_std`/wasm 대응을 공식 지원하면 shim을 걷어낸다

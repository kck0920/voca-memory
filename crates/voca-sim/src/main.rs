//! FSRS 스케줄링 시뮬레이터.
//!
//! 이 바이너리의 산출물은 **코드**가 아니라 **숫자**다. Retention Preset 3개의 예상
//! 일일 부하와 `decks.daily_goal` 기본값을 여기서 정한다.
//! 근거: [`docs/design.md`](../../docs/design.md) Phase 1.
//!
//! `fsrs::simulate`는 rayon을 쓰므로 네이티브 전용이다. 이 도구는 브라우저에
//! 들어가지 않는다. 브라우저에는 `voca-domain`의 [`voca_domain::Scheduler`]만 실린다.

use fsrs::{DEFAULT_PARAMETERS, SimulatorConfig};
use voca_domain::RetentionPreset;

/// 하루에 새로 배우는 단어 수.
///
/// 기본값은 `usize::MAX`인데, 그러면 500단어를 30일 만에 다 쏟아붓고 "학습 구간"이
/// 사라진다. 우리는 중급 학습자의 페이지를 가정한다 — 하루 10단어가 상단이다.
const NEW_CARDS_PER_DAY: usize = 10;

/// 단어 1개 복습에 든 것으로 가정한 평균 시간(초).
const SECONDS_PER_REVIEW: f32 = 12.0;

struct Summary {
    preset: RetentionPreset,
    retention: f32,
    /// 덱을 다 채운 뒤의 안정 구간 일일 리뷰 건수 평균.
    steady_state_reviews: f32,
    /// 같은 구간의 일일 리뷰 건수 최댓값. "하루 최악의 경우"를 UI에 보여줄 때 쓴다.
    peak_reviews: usize,
    /// 마지막 날에 성숙 상태로 분류된 단어 비율.
    matured_share: f32,
    /// 덱을 다 채우는 데 걸린 일수.
    days_to_load: usize,
}

fn simulate_for(preset: RetentionPreset, deck_size: usize) -> Result<Summary, String> {
    let config = SimulatorConfig {
        deck_size,
        learn_limit: NEW_CARDS_PER_DAY,
        review_limit: usize::MAX,
        // 순수 FSRS-6 모델이므로 learning step을 쓰지 않는다. 기본값(2/1)이면
        // 시뮬레이션 결과가 실제 스케줄러와 어긋난다. 근거: docs/adr/0009
        learning_step_count: 0,
        relearning_step_count: 0,
        // Anki 기본 365일은 어휘장에선 과하다. 3년이면 충분하다.
        max_ivl: 1_095.0,
        ..SimulatorConfig::default()
    };

    let result = fsrs::simulate(
        &config,
        &DEFAULT_PARAMETERS,
        preset.desired_retention(),
        Some(0x5EED),
        None,
    )
    .map_err(|e| format!("simulate 실패: {e}"))?;

    Ok(summarize(preset, deck_size, &result))
}

/// 덱을 다 채운 다음부터를 안정 구간으로 본다.
///
/// 앞부분은 새 단어를 배우는 과정이라 리뷰량이 인위적으로 낮다. 그 구간을 평균에
/// 넣으면 부하를 과소평가한다.
fn summarize(
    preset: RetentionPreset,
    deck_size: usize,
    result: &fsrs::SimulationResult,
) -> Summary {
    let reviews = &result.review_cnt_per_day;
    let days_to_load = result
        .learn_cnt_per_day
        .iter()
        .rposition(|n| *n > 0)
        .map(|i| i + 1)
        .unwrap_or(reviews.len());
    let window = &reviews[days_to_load..];

    let steady_state_reviews = if window.is_empty() {
        0.0
    } else {
        window.iter().copied().sum::<usize>() as f32 / window.len() as f32
    };
    let matured = *result.memorized_cnt_per_day.last().unwrap_or(&0.0);

    Summary {
        preset,
        retention: preset.desired_retention(),
        steady_state_reviews,
        peak_reviews: window.iter().copied().max().unwrap_or(0),
        matured_share: matured / deck_size as f32,
        days_to_load,
    }
}

fn report(deck_size: usize, new_per_day: usize, summaries: &[Summary]) {
    println!("덱 {deck_size}단어 · 하루 새 단어 {new_per_day}개 · learning step 없음");
    println!("안정 구간 = 덱을 다 채운 뒤부터 종료일까지");
    println!();

    let header = format!(
        "{:<10} {:>10} {:>13} {:>13} {:>12} {:>14}",
        "프리셋", "retention", "일일 리뷰", "최대 리뷰", "일평균 초", "성숙 단어"
    );
    println!("{header}");
    println!("{}", "-".repeat(header.chars().count()));

    for s in summaries {
        println!(
            "{:<10} {:>10.2} {:>13.1} {:>13} {:>12.0} {:>13.0}%",
            format!("{:?}", s.preset),
            s.retention,
            s.steady_state_reviews,
            s.peak_reviews,
            s.steady_state_reviews * SECONDS_PER_REVIEW,
            s.matured_share * 100.0,
        );
    }

    println!();
    let load = summaries[0].days_to_load;
    println!("덱을 다 채우는 데 {load}일. 일일 시간은 {SECONDS_PER_REVIEW:.0}초/리뷰 환산값.");
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let deck_size: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(500);

    let summaries: Vec<Summary> = RetentionPreset::ALL
        .iter()
        .map(|preset| simulate_for(*preset, deck_size))
        .collect::<Result<_, _>>()?;

    report(deck_size, NEW_CARDS_PER_DAY, &summaries);

    // 프리셋이 서로 의미 있게 다른지 확인한다. 차이가 없으면 세 칸을 노출할 이유가 없다.
    // `RetentionPreset::ALL`은 retention 내림차순이므로 일일 부하도 감소해야 한다.
    for pair in summaries.windows(2) {
        let (higher, lower) = (&pair[0], &pair[1]);
        if higher.steady_state_reviews <= lower.steady_state_reviews {
            return Err(format!(
                "{:?} ({}건/일) 와 {:?} ({}건/일) 이 구분되지 않는다",
                higher.preset,
                higher.steady_state_reviews,
                lower.preset,
                lower.steady_state_reviews
            ));
        }
    }

    Ok(())
}

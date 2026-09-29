//! 기본 시드 단어 데이터 및 초기 적재 로직.
//!
//! 영어 빈도 상위이면서 여러 뜻과 관용구를 가진 핵심 다의어 30개를 제공한다.
//! 저작권 문제가 없도록 직접 작성한 정의와 예문을 사용한다 ([`docs/adr/0007`]).

use voca_store::{
    DictionarySense, NewDeck, SenseKind, Store, StoreResult, UpsertDictionaryWord, UserId,
};
use voca_store_sqlite::SqliteStore;

fn s(pos: &str, def: &str, ex: &str) -> DictionarySense {
    DictionarySense {
        kind: SenseKind::Word,
        pos: Some(pos.to_owned()),
        definition: def.to_owned(),
        example_en: Some(ex.to_owned()),
        example_ko: None,
    }
}

fn word(lemma: &str, phonetic: &str, senses: Vec<DictionarySense>) -> UpsertDictionaryWord {
    UpsertDictionaryWord {
        lemma: lemma.to_owned(),
        phonetic: Some(phonetic.to_owned()),
        senses,
    }
}

/// 라이선스 안전한 기본 시드 단어 30개 목록을 돌려준다.
pub fn seed_words() -> Vec<UpsertDictionaryWord> {
    vec![
        word("run", "/rʌn/", vec![
            s("verb", "달리다, 빠르게 이동하다", "She runs five miles every morning."),
            s("verb", "운영하다, 경영하다", "He has been running a software startup."),
            s("verb", "(프로그램 등이) 실행되다, 작동하다", "The server runs continuously in the background."),
        ]),
        word("keep", "/kiːp/", vec![
            s("verb", "유지하다, 계속 ~인 상태로 있다", "Keep calm and focused on your goals."),
            s("verb", "보관하다, 간직하다", "You can keep the change."),
            s("verb", "(약속, 비밀 등을) 지키다", "She always keeps her promises."),
        ]),
        word("make", "/meɪk/", vec![
            s("verb", "만들다, 제작하다", "They make high-quality mechanical keyboards."),
            s("verb", "~하게 만들다, ~하게 하다", "The music made her feel nostalgic."),
            s("verb", "(결정, 약속, 노력 등을) 하다", "We need to make an important decision today."),
        ]),
        word("take", "/teɪk/", vec![
            s("verb", "가지고 가다, 잡다", "Take an umbrella in case it rains."),
            s("verb", "(시간이나 노력이) 걸리다", "It takes about thirty minutes on foot."),
            s("verb", "(시험, 수업 등을) 치르다, 받다", "I am going to take an English proficiency test."),
        ]),
        word("hold", "/həʊld/", vec![
            s("verb", "손에 잡다, 쥐다", "She held the cup with both hands."),
            s("verb", "(행사, 회의 등을) 개최하다", "The annual conference will be held next month."),
            s("verb", "기다리다 (전화 등)", "Please hold the line for a moment."),
        ]),
        word("turn", "/tɜːn/", vec![
            s("verb", "돌리다, 회전하다, 방향을 바꾸다", "Turn right at the next intersection."),
            s("verb", "~한 상태로 변하다, 바뀌다", "The leaves turn red and yellow in autumn."),
            s("noun", "차례, 순번", "It is your turn to roll the dice."),
        ]),
        word("break", "/breɪk/", vec![
            s("verb", "깨뜨리다, 부수다, 고장나다", "Be careful not to break the fragile glass."),
            s("verb", "(규칙, 법, 약속 등을) 어기다", "Never break the traffic safety rules."),
            s("noun", "휴식 시간, 쉬는 시간", "Let's take a ten-minute coffee break."),
        ]),
        word("point", "/pɔɪnt/", vec![
            s("noun", "요점, 핵심, 주장", "That is a very valid and thoughtful point."),
            s("verb", "손가락 등으로 가리키다", "He pointed at the distant mountain peak."),
            s("noun", "지점, 시점, 점수", "At this point in time, we have no other options."),
        ]),
        word("clear", "/klɪər/", vec![
            s("adjective", "분명한, 명확한, 알기 쉬운", "Her explanation was perfectly clear to everyone."),
            s("adjective", "맑은, 투명한", "The sky is completely clear without any clouds."),
            s("verb", "치우다, 비우다, 장애물을 없애다", "Please clear your desk before leaving the office."),
        ]),
        word("close", "/kləʊz/", vec![
            s("verb", "닫다, 문을 닫다", "Please close the door gently behind you."),
            s("adjective", "(거리상) 가까운, 근처의", "The subway station is very close to our house."),
            s("adjective", "(관계가) 친밀한, 절친한", "They have been close friends since childhood."),
        ]),
        word("stand", "/stænd/", vec![
            s("verb", "서다, 서 있다", "All passengers were standing in the crowded train."),
            s("verb", "참다, 견디다 (주로 부정문/의문문)", "I cannot stand this extreme summer heat."),
            s("noun", "입장, 태도", "The government took a firm stand against corruption."),
        ]),
        word("call", "/kɔːl/", vec![
            s("verb", "전화하다, 연락하다", "I will call you back in a few minutes."),
            s("verb", "부르다, 칭하다", "What do you call this tool in English?"),
            s("noun", "결정, 판단", "It was a tough call, but we made the right choice."),
        ]),
        word("check", "/tʃek/", vec![
            s("verb", "살펴보다, 점검하다, 확인하다", "Always check your code before pushing to git."),
            s("noun", "수표, 청구서", "May we have the check, please?"),
            s("verb", "(짐을) 부치다, 맡기다", "You can check two bags on this international flight."),
        ]),
        word("leave", "/liːv/", vec![
            s("verb", "떠나다, 출발하다", "The bus leaves the terminal at noon."),
            s("verb", "남기다, 두고 오다", "Did you leave your keys on the counter?"),
            s("noun", "휴가, 떠나 있음", "He is currently on annual leave."),
        ]),
        word("order", "/ˈɔːdə/", vec![
            s("verb", "(물건, 음식 등을) 주문하다", "Are you ready to order your dinner?"),
            s("noun", "순서, 질서, 정리된 상태", "Arrange these index cards in alphabetical order."),
            s("verb", "명령하다, 지시하다", "The commander ordered the team to retreat."),
        ]),
        word("charge", "/tʃɑːdʒ/", vec![
            s("verb", "(요금을) 청구하다", "They charge a small delivery fee for groceries."),
            s("verb", "(배터리를) 충전하다", "Remember to charge your phone before going out."),
            s("noun", "책임, 담당", "Who is in charge of this project?"),
        ]),
        word("present", "/ˈpreznt/", vec![
            s("noun", "선물", "She gave me a lovely birthday present."),
            s("adjective", "현재의, 출석한, 참석한", "All team members were present at the meeting."),
            s("verb", "제시하다, 발표하다, 보여주다", "He will present the quarterly earnings report."),
        ]),
        word("sound", "/saʊnd/", vec![
            s("noun", "소리, 음향", "The strange sound came from the attic."),
            s("verb", "~처럼 들리다", "That sounds like an amazing opportunity."),
            s("adjective", "건전한, 건강한, 타당한", "They made a sound financial investment."),
        ]),
        word("strike", "/straɪk/", vec![
            s("verb", "치다, 부딪히다", "Lightning struck the tall pine tree."),
            s("noun", "파업", "The railway workers went on strike for better pay."),
            s("verb", "(생각, 영감이) 떠오르다", "An ingenious idea suddenly struck him."),
        ]),
        word("bear", "/beə/", vec![
            s("verb", "견디다, 감당하다", "He could hardly bear the emotional pain."),
            s("noun", "곰", "A wild brown bear was spotted near the river."),
            s("verb", "(열매를) 맺다, 낳다", "These apple trees bear sweet fruit every autumn."),
        ]),
        word("draw", "/drɔː/", vec![
            s("verb", "(그림을) 그리다", "She loves to draw portraits with charcoal."),
            s("verb", "끌어당기다, 끌다", "The exhibition drew thousands of enthusiastic visitors."),
            s("noun", "무승부, 비김", "The soccer match ended in a thrilling two-two draw."),
        ]),
        word("drive", "/draɪv/", vec![
            s("verb", "운전하다, 몰다", "She drives to work every weekday."),
            s("verb", "(특정 상태로) 몰아가다", "The constant noise was driving him crazy."),
            s("noun", "추진력, 의욕", "He possesses great drive and personal ambition."),
        ]),
        word("lead", "/liːd/", vec![
            s("verb", "이끌다, 안내하다, 지휘하다", "She led the research team to significant breakthroughs."),
            s("verb", "(결과로) 이어지다, 초래하다", "Poor dietary habits can lead to health problems."),
            s("noun", "선두, 우위", "The candidate holds a substantial lead in recent polls."),
        ]),
        word("record", "/ˈrekɔːd/", vec![
            s("verb", "기록하다, 녹음하다", "Please record your daily study hours in the log."),
            s("noun", "기록, 음반", "She broke the world track record yesterday."),
        ]),
        word("catch", "/kætʃ/", vec![
            s("verb", "잡다, 받다", "He managed to catch the ball with one bare hand."),
            s("verb", "(기차, 버스 등을) 타다", "Hurry up if you want to catch the last express train."),
            s("verb", "(감기 등에) 걸리다", "Dress warmly so that you do not catch a cold."),
        ]),
        word("fall", "/fɔːl/", vec![
            s("verb", "떨어지다, 넘어지다", "Raindrops began to fall quietly on the roof."),
            s("verb", "(온도, 가격 등이) 떨어지다, 하락하다", "Temperatures will fall drastically tonight."),
            s("noun", "가을 (미국 영어)", "The campus looks stunningly beautiful in the fall."),
        ]),
        word("pass", "/pɑːs/", vec![
            s("verb", "지나가다, 통과하다", "Years pass by quickly when you are occupied."),
            s("verb", "(시험 등에) 합격하다", "She worked very hard and passed the bar examination."),
            s("verb", "(물건을) 건네주다", "Could you please pass the salt?"),
        ]),
        word("issue", "/ˈɪʃuː/", vec![
            s("noun", "쟁점, 문제", "Climate change is a critical global issue."),
            s("verb", "발행하다, 발급하다", "The embassy issued a temporary passport."),
            s("noun", "(잡지, 신문의) 호", "Did you read the latest issue of the magazine?"),
        ]),
        word("open", "/ˈəʊpən/", vec![
            s("verb", "열다, 펼치다", "Open the window to let some fresh air in."),
            s("adjective", "열려 있는, 영업 중인", "The pharmacy is open twenty-four hours a day."),
        ]),
        word("play", "/pleɪ/", vec![
            s("verb", "(놀이나 게임, 스포츠를) 하다", "The children love to play soccer after school."),
            s("verb", "(악기를) 연주하다, (음악을) 재생하다", "He plays the acoustic guitar beautifully."),
            s("noun", "연극, 희곡", "We went to see a modern Shakespeare play."),
        ]),
    ]
}

/// 저장소에 시드 단어를 로드한다. 이미 존재하는 단어는 무시하거나 갱신한다.
pub async fn load_seeds_into_store(store: &SqliteStore) -> StoreResult<usize> {
    let seeds = seed_words();
    let mut count = 0;
    for seed in seeds {
        if store.upsert_dictionary_word(seed).await.is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

/// 신규 가입 사용자를 위해 기본 덱과 초기 시드 카드를 생성한다.
pub async fn bootstrap_user_cards(store: &SqliteStore, user_id: UserId) -> StoreResult<()> {
    // 1. 이미 덱이 있는지 확인
    let decks = store.list_decks(user_id).await?;
    let deck_id = if let Some(existing) = decks.first() {
        existing.id
    } else {
        // 기본 덱 생성
        let deck = store
            .create_deck(
                user_id,
                NewDeck {
                    name: "기본 필수 다의어 30".to_owned(),
                    description: Some("가장 빈도가 높고 여러 뜻을 가진 핵심 단어 모음".to_owned()),
                    daily_goal: Some(20),
                    new_per_day: Some(10),
                },
            )
            .await?;
        deck.id
    };

    // 2. 시드 단어들에서 Sense ID를 모아 덱에 카드로 추가
    let mut sense_ids = Vec::new();
    let seeds = seed_words();
    for seed in &seeds {
        if let Ok(Some(word)) = store.find_word(user_id, &seed.lemma).await {
            for sense in word.senses {
                sense_ids.push(sense.id);
            }
        }
    }

    if !sense_ids.is_empty() {
        // 최대 30개 Sense를 카드화
        let slice = if sense_ids.len() > 30 {
            &sense_ids[..30]
        } else {
            &sense_ids[..]
        };
        let _ = store.add_cards(user_id, deck_id, slice).await?;
    }

    Ok(())
}

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use crate::AppState;
use crate::routes::{ApiError, LoggedIn, require_login};

/// 사용자가 직접 시드 덱 생성을 요청할 때 부르는 핸들러.
pub async fn bootstrap_seed_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    state.require_same_origin(&headers)?;
    let LoggedIn { user, .. } = require_login(&state, &headers).await?;
    bootstrap_user_cards(state.store(), user.user_id)
        .await
        .map_err(ApiError::store)?;
    Ok(axum::http::StatusCode::OK.into_response())
}

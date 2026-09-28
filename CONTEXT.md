# Voca Memory

영어 어휘를 장기 기억에 남기도록 복습을 스케줄링하는 학습 앱. 이 파일은 **용어집**이다. 설계 결정은 [`docs/adr/`](./docs/adr/), 전체 계획은 [`docs/design.md`](./docs/design.md).

코드에서는 영어를 쓰고, 아래 정의된 대문자 단어는 번역하지 않고 그대로 식별자로 쓴다.

## 어휘

**Word** (어 항목):
사전에 존재하는 어휘 단위. 하나의 표기형(lemma)과 하나 이상의 Sense로 이루어진다. `run`이 Word다.
_Avoid_: Card, Term, Item, Entry, Vocabulary

**Sense** (의명 하나의 뜻):
Word가 가지는 하나의 뜻 단위. 사전의 뜻, 관용구(phrasal verb), 특정 문맥에서의 쓰임, 학습자가 직접 지은 뜻이 모두 Sense가 된다. Word는 Sense를 하나 이상 가지며, Card를 쪼갼 때의 기준이 된다. Sense의 종류(`word` / `phrase` / `example`)가 Card의 어느 면을 보여줄지 정한다.
_Avoid_: Definition (뜻풀이 문장과 다름), Meaning, Pos, Word (개별 뜻이 아니라 어 항목)

**Card** (학습 단위):
학습자가 복습하는 단위. 정확히 하나의 Sense를 향하며, 복습 스케줄링과 통계의 대상이다. 하나의 Word에 여러 Card가 대응될 수 있다.
_Avoid_: Note, Flashcard, Item, Vocabulary

**Card Front** (카드의 앞면):
학습자가 의미를 되돌리기 전에 보는 것. Sense의 종류가 정한다 — `word`·`phrase`는 그 Word의 표기형을, `example`은 그 Sense의 예문을 보여준다. Sense는 언제나 어떤 Word에 속하므로, 예문 Sense에도 Word가 있다.
_Avoid_: Prompt, Question, Side

**Deck** (덱):
Card의 명명된 묶음. 하나의 학습 목표를 나타낸다. Card는 정확히 하나의 Deck에 속한다.
_Avoid_: Collection, List, Set, Book, Folder

> **명시적 금지 — Card의 다중 Deck 소속.** 하나의 Card가 두 개 이상의 Deck에 동시에 속하지 않는다. "수능 단어" 덱의 단어를 "TOEFL 핵심"에도 넣고 싶다면 Card를 복제한다. 복제본은 **독립된 Memory State**를 가진다 — 한 덱에서 외운 기억이 다른 덱에 자동 이월되지 않는다. 이건 Anki와 같은 선택이며, 이월을 원하면 명시적으로 "Deck 간 전이" 기능을 추가해야 한다.

## 복습

**Rating** (평가):
학습자가 Card를 본 직후 매기는 4단계 평가. `Again` / `Hard` / `Good` / `Easy`.
_Avoid_: Grade, Score, Result, Verdict

**Review** (복습):
Card를 제시하고 Rating을 받는 한 번의 상호작용. 학습 행위의 최소 단위이자 Streak 집계의 대상이다.
_Avoid_: Attempt, Answer, Session, Study, Drill

**Memory State** (기억 상태):
스케줄러가 Card에 대해 관리하는 스칼라 상태의 묶음. Stability·Difficulty·반복 횟수·누적 실패 수를 담는다. Card 본문과 분리되어 Card와 1:1로 대응한다.
_Avoid_: Progress, Stats, CardState (하위 필드와 통용어가 섞임)

**Stability** (안정성):
스케줄러가 판단한, 현재 기억이 유지될 것으로 보이는 기간(일 단위). 복습할 때마다 지수적으로 증가하고, 실패하면 급격히 떨어진다.
_Avoid_: Retention, Strength, Interval

**Difficulty** (난이도):
스케줄러가 판단한, 그 Card를 다시 떠올리는 데 드는 어려움. Stability의 증가 배율을 낮춘다.
_Avoid_: Hardness, Complexity

**Retention Preset** (기억 유지 강도):
사용자가 고르는 목표 기억 유지율 프리셋. `부지런` / `균형` / `아끼기` 세 가지뿐이고, 각각 FSRS의 `desired_retention`을 다른 값으로 고정한다. 연속값으로 노출하지 않는다.
_Avoid_: Intensity, Mode, Difficulty, Setting

**Due** (복습 대상):
`now >= due_at`을 만족하는 Card의 상태. Card에 저장되는 플래그가 아니라 시각 비교의 **결과**다.
_Avoid_: Scheduled (저장된 예약 시각과 혼동), Pending, Ready

**Mastery** (숙련도):
특정 Word 또는 Card가 얼마나 확고하게 습득되었는지를 나타내는 파생 지표. 저장하지 않고 Memory State에서 계산한다. 계정 Level과 혼동하지 않는다.
_Avoid_: Level, Progress, Fluency

## 학습 활동

**Session** (세션):
한 번의 학습 시도를 묶는 개념. UI에서는 쓰지만 **영속 집계 대상이 아니다** — Streak 판정에 세션 목표 달성 여부를 쓰지 않는다.
_Avoid_: Study, Round, Run, Batch

**Streak** (연속 학습 일수):
하루에 Review가 1건 이상 있었던 날들이 이어진 구간. Rating이 무엇이든 — Again을 눌러 실패한 것만으로도 그날은 이어진다. 날짜 경계는 **사용자 현지 시각** 기준이다.
_Avoid_: Chain, Combo, Attendance, Cadence

**XP** (경험치):
Review 활동에서 파생되는 계정 단위 진행 점수. 정확한 산식은 미정.
_Avoid_: Score, Points, Reward

**Level** (레벨):
누적 XP에서 파생되는 계정 단위 등급. 어떤 Card나 Word가 얼마나 어려운지와는 무관하다.
_Avoid_: Rank, Tier, Grade

## 음성

**Audio** (음성 자산):
Word에 붙는 **재생 전용** 음성 파일. 사전에 미리 녹음된 음성이나 합성(TTS) 결과를 캐시한다.
_Avoid_: Pronunciation (평가와 혼동), Voice, Sound

**Pronunciation Score** (발음 점수):
학습자가 직접 낸 발음을 채점한 결과. **초기 범위 밖**이며 Audio 서브시스템에 포함되지 않는다. 별도 서브시스템으로 다룬다.
_Avoid_: Audio, Speech, Accuracy, Fluency

## 동기화

**Revision** (리비전):
동기화 대상 행이 변경될 때마다 증가하는 단조 카운터. 어떤 클라이언트가 마지막으로 본 버전을 식별하는 데 쓴다.
_Avoid_: Version (엔지니어링 용어와 충돌), ETag

# Card의 앞면은 Sense의 kind가 정한다

`Sense.kind`가 무엇을 Card의 앞면에 보여줄지 정한다. `senses.word_id`는 **항상 NOT NULL**이고, `example` kind도 예외가 아니다.

| `kind` | Card 앞면 | Sense가 속하는 Word |
|---|---|---|
| `word` | `words.lemma` | 그 단어 자신 |
| `phrase` | `words.lemma` | 그 구문 자신이 Word인 새 행 (`run off`) |
| `example` | `senses.example_en` | **설명되는 원래 단어** (`abandon`) |

## 이게 왜 깔끔한가

`example` kind가 답을 만들었다. "이 문장에서 `abandon`은 뜻이 다"는 학습에서, **문장은 Word가 아니다.** Word는 언제나 "말씀되는 단어"고, `example` Sense는 그 단어가 특정 문맥에서 갖는 뜻이다. Card가 그 문장을 보여주는 것뿐이다.

그래서 Word 정의를 "사전에 존재하는 어휘 단위"로 유지하면서도 문장 Sense를 표현할 수 있다. `Word(lemma="abandon")` 아래에 `Sense(kind=example, example_en="They abandoned the car and fled.")`가 붙는다.

`phrase`만 새 Word를 요구한다. `run off`은 실제로 별도의 표기형이므로 `Word(lemma="run off")` 행이 생기고 거기에 Sense가 붙는다. 이건 정상이지만 단어 목록에 구문이 섞인다는 뜻이다.

## 대안의 함정

- **문장도 Word로 만든다** (`words.lemma`에 문장 전체). 스키마는 가장 단순해지지만 Word의 정의가 깨지고, 단어 검색/브라우즈에 문장이 섞인다. 한 단어에 Sense 5개가 붙는 것과는 차원이 다르다 — 검색 대상 자체가 달라진다.
- **`senses.word_id`를 nullable로.** 그러면 "어떤 단어의 뜻도 아닌 Sense"가 생기고, Sense가 두 갈래로 갈라진다.
- **별도 `examples` 테이블.** `Card`가 `sense_id`와 `example_id`를 모두 가질 수 있어야 하므로 foreign key가 둘이 된다. 스케줄링·통계·Mastery 계산 전체가 분기를 탄다. [ADR-0006](./0006-sense-covers-more-than-dictionary-definitions.md)이 이미 이를 기각했다.

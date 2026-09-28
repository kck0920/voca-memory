# learning step을 쓰지 않는다 — 순수 FSRS-6

Learning step과 relearning step을 **0으로 고정한다.** 순수 FSRS-6 모델이다.

## 무엇을 포기하는가

Anki에서 `Again`을 누르면 그 단어를 같은 세션 안에서 몇 번 더 보여준다 (1분, 10분). `Again`을 3~4번 눌러 단어를 끝까지 외우는 경험이다. 이건 learning step 기계가 만드는 것이고, `card_states.state`는 `new | learning | review | relearning` 네 단계를 갖게 된다.

우리는 이것을 갖지 않는다. 대신:

- `card_states.state`는 `new | review` **두 값뿐**이다
- `Again` 직후의 재제시는 `scheduled_days < 1` 조건이 만든다. 서버가 이 조건으로 세션 내 큐를 만든다
- 최소 간격은 1분

## 왜

**FSRS-6은 learning step을 모델링하지 않는다.** `fsrs` 크레이트의 `next_states`는 한 번의 step만 계산한다. step을 흉내내려면 알고리즘을 감싸야 하는데, 그러면 스케줄러가 검증되지 않은 코드가 된다.

여기에 측정했다. **성숙도에 따라 `Again`의 의미가 갈린다.**

| Card 상태 | `Again` 후 interval |
|---|---|
| 새 단어 | 0.21일 (약 5분) |
| 성숙한 단어 (Stability 30일) | 2.3일 |

새 단어를 틀리면 오늘 다시 만나지만, 굳어진 단어를 잊으면 이틀 뒤에 다시 만난다. 이것이 FSRS-6의 판단이고, 그 단어는 이미 충분할 만큼 굳어졌다는 뜻이다.

`Again = 0 XP`라는 결정과도 일관된다. learning step을 넣으면 한 번의 학습이 3~4번의 Review로 세어져 XP 1:1 대응이 깨진다. 복습 단위와 XP 단위를 같게 두는 편이 낫다.

## 사용자 대우

"또 모름"을 눌렀는데 카드가 사라지는 것이 버그처럼 보일 수 있다. 이건 **UI로 고칠 수 없다** — 알고리즘을 바꿔야 한다. 그러므로 study 화면에 "이 단어는 내일 다시 만납니다"를 노출해 Understandable하게 만든다. 되돌리고 싶다면 learning step을 도입해야 하고, 그건 XP 1:1을 재설계해야 한다.

## 대안의 함정

"시뮬레이션이 잘 나오니까 learning step을 넣자." 안 된다. `fsrs::SimulatorConfig::default()`는 `learning_step_count: 2, relearning_step_count: 1`이라 **learning step이 있는 것처럼** 시뮬레이션한다. 이 기본값을 그대로 쓰면 시뮬레이션 숫자가 실제 스케줄러와 어긋난다 — 측정 결과가 근거가 아니라 fiction이 된다. `voca-sim`이 `0`으로 고정하는 것은 이 때문이다.

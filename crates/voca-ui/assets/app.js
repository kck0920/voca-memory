// Voca Memory 브라우저 인터랙션 스크립트.
//
// 1. 단어 발음 듣기 (Web Speech API TTS)
// 2. "오늘 더 학습하기" 및 일일 학습량 설정 (new_per_day / daily_goal)
// 3. 내 단어장 카드 목록 조회, 검색, 삭제 (My Cards Library)
// 4. 영영사전 한글 뜻 원클릭 번역 (MyMemory Translation)
// 5. 레벨 칭호 (Title) 및 최근 7일 학습 통계 차트

(function () {
  'use strict';

  const ORIGIN = window.location.origin;

  // ── API 헬퍼 ──────────────────────────────────────────────
  async function api(path, options = {}) {
    const defaultHeaders = {
      'Origin': ORIGIN,
      'Content-Type': 'application/json',
    };
    const res = await fetch(path, {
      ...options,
      headers: {
        ...defaultHeaders,
        ...(options.headers || {}),
      },
    });
    return res;
  }

  // ── 상태 ──────────────────────────────────────────────────
  let currentUser = null;
  let queueCards = [];
  let currentCardIndex = 0;
  let isCardFlipped = false;
  let activeDeckId = null;
  let allUserCards = [];

  // ── TTS 음성 발음 재생 (원어민 오디오 스트림 1순위 + Web Speech fallback) ──
  let activeAudio = null;
  let activeUtterance = null; // GC 가비지 컬렉션 방지용

  function speakWord(text) {
    if (!text) return;
    const cleanText = text.trim();
    if (!cleanText) return;

    try {
      if (activeAudio) {
        activeAudio.pause();
        activeAudio.currentTime = 0;
        activeAudio = null;
      }

      // 1순위: 서버 오디오 프록시 (/api/audio/tts?text=...)
      const audioUrl = `/api/audio/tts?text=${encodeURIComponent(cleanText)}`;
      const audio = new Audio(audioUrl);
      activeAudio = audio;

      const playPromise = audio.play();
      if (playPromise !== undefined) {
        playPromise.catch(() => {
          // 서버 실패 시 2순위: Google TTS 직접 호출
          const directUrl = `https://translate.google.com/translate_tts?ie=UTF-8&tl=en&client=tw-ob&q=${encodeURIComponent(cleanText)}`;
          const fallbackAudio = new Audio(directUrl);
          activeAudio = fallbackAudio;
          fallbackAudio.play().catch(() => {
            // 3순위: Web Speech API fallback
            speakWithSpeechSynthesis(cleanText);
          });
        });
      }
    } catch (_) {
      speakWithSpeechSynthesis(cleanText);
    }
  }

  function speakWithSpeechSynthesis(text) {
    if (!('speechSynthesis' in window)) return;
    try {
      window.speechSynthesis.resume();
      window.speechSynthesis.cancel();
      setTimeout(() => {
        const u = new SpeechSynthesisUtterance(text);
        u.lang = 'en-US';
        u.rate = 0.92;
        const voices = window.speechSynthesis.getVoices() || [];
        const enVoice = voices.find(
          (v) =>
            v.lang.startsWith('en') &&
            (v.name.includes('Google') ||
              v.name.includes('Natural') ||
              v.name.includes('Samantha') ||
              v.name.includes('US'))
        );
        if (enVoice) u.voice = enVoice;

        activeUtterance = u;
        u.onend = () => {
          activeUtterance = null;
        };
        u.onerror = () => {
          activeUtterance = null;
        };
        window.speechSynthesis.speak(u);
      }, 10);
    } catch (_) {}
  }

  // ── 레트로 8-Bit Web Audio API 효과음 엔진 ────────────────
  let audioCtx = null;
  let sfxEnabled = localStorage.getItem('voca_sfx_enabled') !== 'false';

  function getAudioContext() {
    if (!audioCtx) {
      const AudioContext = window.AudioContext || window.webkitAudioContext;
      if (AudioContext) audioCtx = new AudioContext();
    }
    if (audioCtx && audioCtx.state === 'suspended') {
      audioCtx.resume();
    }
    return audioCtx;
  }

  function playSfx(type) {
    if (!sfxEnabled) return;
    try {
      const ctx = getAudioContext();
      if (!ctx) return;
      const now = ctx.currentTime;

      if (type === 'click') {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'square';
        osc.frequency.setValueAtTime(520, now);
        osc.frequency.exponentialRampToValueAtTime(780, now + 0.05);
        gain.gain.setValueAtTime(0.08, now);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.05);
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.start(now);
        osc.stop(now + 0.05);
      } else if (type === 'flip') {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'triangle';
        osc.frequency.setValueAtTime(280, now);
        osc.frequency.exponentialRampToValueAtTime(880, now + 0.08);
        gain.gain.setValueAtTime(0.12, now);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.08);
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.start(now);
        osc.stop(now + 0.08);
      } else if (type === 'good' || type === 'easy') {
        [523.25, 659.25].forEach((freq, i) => {
          const osc = ctx.createOscillator();
          const gain = ctx.createGain();
          osc.type = 'square';
          osc.frequency.setValueAtTime(freq, now + i * 0.07);
          gain.gain.setValueAtTime(0.07, now + i * 0.07);
          gain.gain.exponentialRampToValueAtTime(0.001, now + i * 0.07 + 0.12);
          osc.connect(gain);
          gain.connect(ctx.destination);
          osc.start(now + i * 0.07);
          osc.stop(now + i * 0.07 + 0.12);
        });
      } else if (type === 'hard') {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'square';
        osc.frequency.setValueAtTime(440, now);
        osc.frequency.exponentialRampToValueAtTime(370, now + 0.1);
        gain.gain.setValueAtTime(0.08, now);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.1);
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.start(now);
        osc.stop(now + 0.1);
      } else if (type === 'again') {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'sawtooth';
        osc.frequency.setValueAtTime(240, now);
        osc.frequency.exponentialRampToValueAtTime(160, now + 0.12);
        gain.gain.setValueAtTime(0.09, now);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.12);
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.start(now);
        osc.stop(now + 0.12);
      } else if (type === 'victory') {
        [523.25, 659.25, 783.99, 1046.50].forEach((freq, i) => {
          const osc = ctx.createOscillator();
          const gain = ctx.createGain();
          osc.type = 'square';
          osc.frequency.setValueAtTime(freq, now + i * 0.09);
          gain.gain.setValueAtTime(0.09, now + i * 0.09);
          gain.gain.exponentialRampToValueAtTime(0.001, now + i * 0.09 + 0.2);
          osc.connect(gain);
          gain.connect(ctx.destination);
          osc.start(now + i * 0.09);
          osc.stop(now + i * 0.09 + 0.2);
        });
      }
    } catch (_) {}
  }

  function setupThemeAndSfx() {
    const savedTheme = localStorage.getItem('voca_theme') || 'arcade';
    document.documentElement.setAttribute('data-theme', savedTheme);
    const themeSelect = document.getElementById('theme-select');
    if (themeSelect) {
      themeSelect.value = savedTheme;
      themeSelect.addEventListener('change', (e) => {
        const theme = e.target.value;
        document.documentElement.setAttribute('data-theme', theme);
        localStorage.setItem('voca_theme', theme);
        playSfx('click');
      });
    }

    const sfxBtn = document.getElementById('sfx-toggle-btn');
    const sfxIcon = document.getElementById('sfx-icon');
    const sfxLabel = document.getElementById('sfx-label');

    function updateSfxUi() {
      if (sfxBtn) {
        if (sfxEnabled) {
          sfxBtn.classList.remove('muted');
          if (sfxIcon) sfxIcon.textContent = '🔊';
          if (sfxLabel) sfxLabel.textContent = 'SFX';
        } else {
          sfxBtn.classList.add('muted');
          if (sfxIcon) sfxIcon.textContent = '🔇';
          if (sfxLabel) sfxLabel.textContent = 'MUTED';
        }
      }
    }
    updateSfxUi();

    sfxBtn?.addEventListener('click', () => {
      sfxEnabled = !sfxEnabled;
      localStorage.setItem('voca_sfx_enabled', sfxEnabled);
      updateSfxUi();
      if (sfxEnabled) playSfx('click');
    });
  }

  // ── 레벨 칭호 매핑 ────────────────────────────────────────
  function getLevelTitle(level) {
    const titles = {
      1: '단어 입문자',
      2: '어휘 탐색가',
      3: '단어 수집가',
      4: '어휘 실천가',
      5: '단어 숙련자',
      6: '어휘 탐구자',
      7: '어휘 마스터',
      8: '기억의 연금술사',
      9: '언어의 건축가',
    };
    return titles[level] || '기억의 현자';
  }

  // ── 초기화 ────────────────────────────────────────────────
  document.addEventListener('DOMContentLoaded', init);

  async function init() {
    setupThemeAndSfx();
    setupAuthTabs();
    setupAuthForms();
    setupActionNav();
    setupDictSearch();
    setupCustomSenseForm();
    setupKeyboardShortcuts();
    setupSeedButton();
    setupDeckSettingsForm();
    setupCardsSearch();

    await checkAuthStatus();
  }

  // ── 인증 상태 확인 ────────────────────────────────────────
  async function checkAuthStatus() {
    try {
      const res = await api('/api/auth/me', { method: 'GET' });
      if (res.ok) {
        const data = await res.json();
        if (data && data.user_id) {
          currentUser = data;
          onLoggedIn(data);
          return;
        }
      }
    } catch (e) {
      console.error('인증 상태 확인 실패:', e);
    }
    onLoggedOut();
  }

  function onLoggedIn(user) {
    const authStatus = document.getElementById('auth-status');
    const authSection = document.getElementById('auth-section');
    const studyPanel = document.getElementById('study-panel');

    if (authStatus) {
      authStatus.innerHTML = `
        <span class="user-badge"><span class="badge-dot">●</span> <strong>${escapeHtml(user.display_name)}</strong> 님</span>
        <button type="button" id="logout-btn" class="mini-btn">로그아웃</button>
      `;
      document.getElementById('logout-btn')?.addEventListener('click', handleLogout);
    }

    if (authSection) authSection.classList.add('hidden');
    if (studyPanel) studyPanel.classList.remove('hidden');

    loadStudyQueue();
  }

  function onLoggedOut() {
    const authStatus = document.getElementById('auth-status');
    const authSection = document.getElementById('auth-section');
    const studyPanel = document.getElementById('study-panel');

    if (authStatus) {
      authStatus.innerHTML = `<span class="guest-badge">로그인이 필요합니다</span>`;
    }

    if (authSection) authSection.classList.remove('hidden');
    if (studyPanel) studyPanel.classList.add('hidden');
  }

  // ── 인증 폼 및 탭 ────────────────────────────────────────
  function setupAuthTabs() {
    const loginTab = document.getElementById('tab-login-btn');
    const regTab = document.getElementById('tab-register-btn');
    const loginForm = document.getElementById('login-form');
    const regForm = document.getElementById('register-form');
    const authMsg = document.getElementById('auth-msg');

    if (!loginTab || !regTab) return;

    loginTab.addEventListener('click', () => {
      loginTab.classList.add('active');
      regTab.classList.remove('active');
      loginForm?.classList.remove('hidden');
      regForm?.classList.add('hidden');
      if (authMsg) authMsg.textContent = '';
    });

    regTab.addEventListener('click', () => {
      regTab.classList.add('active');
      loginTab.classList.remove('active');
      regForm?.classList.remove('hidden');
      loginForm?.classList.add('hidden');
      if (authMsg) authMsg.textContent = '';
    });
  }

  function setupAuthForms() {
    const loginForm = document.getElementById('login-form');
    const regForm = document.getElementById('register-form');
    const authMsg = document.getElementById('auth-msg');

    loginForm?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const email = document.getElementById('login-email')?.value.trim();
      const password = document.getElementById('login-password')?.value;

      if (!email || !password) return;
      showMsg(authMsg, '로그인 처리 중...', 'info');

      try {
        const res = await api('/api/auth/login', {
          method: 'POST',
          body: JSON.stringify({ email, password }),
        });

        if (res.ok) {
          showMsg(authMsg, '로그인 성공! 대시보드로 이동합니다...', 'success');
          setTimeout(() => window.location.reload(), 300);
        } else {
          const err = await res.json().catch(() => ({ message: '로그인 실패' }));
          showMsg(authMsg, `[오류] ${err.message || '이메일 또는 비밀번호가 올바르지 않습니다.'}`, 'error');
        }
      } catch (err) {
        showMsg(authMsg, '[오류] 서버와 통신할 수 없습니다.', 'error');
      }
    });

    regForm?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const email = document.getElementById('reg-email')?.value.trim();
      const displayName = document.getElementById('reg-name')?.value.trim();
      const password = document.getElementById('reg-password')?.value;

      if (!email || !displayName || !password) return;
      showMsg(authMsg, '회원가입 처리 중...', 'info');

      try {
        const res = await api('/api/auth/register', {
          method: 'POST',
          body: JSON.stringify({
            email,
            display_name: displayName,
            password,
            timezone: 'Asia/Seoul',
            retention: 'balanced',
          }),
        });

        if (res.ok) {
          showMsg(authMsg, '회원가입 성공! 환영합니다.', 'success');
          setTimeout(() => window.location.reload(), 300);
        } else {
          const err = await res.json().catch(() => ({ message: '회원가입 실패' }));
          showMsg(authMsg, `[오류] ${err.message || '회원가입에 실패했습니다.'}`, 'error');
        }
      } catch (err) {
        showMsg(authMsg, '[오류] 서버와 통신할 수 없습니다.', 'error');
      }
    });
  }

  async function handleLogout() {
    try {
      await api('/api/auth/logout', { method: 'POST', body: '{}' });
    } catch (_) {}
    window.location.reload();
  }

  // ── 패널 탭 네비게이션 ────────────────────────────────────
  function setupActionNav() {
    const tabs = document.querySelectorAll('.action-tab');
    tabs.forEach((tab) => {
      tab.addEventListener('click', () => {
        playSfx('click');
        tabs.forEach((t) => t.classList.remove('active'));
        tab.classList.add('active');

        const target = tab.dataset.tab;
        document.querySelectorAll('.tab-content').forEach((c) => {
          c.classList.add('hidden');
          c.classList.remove('active');
        });

        const activeContent = document.getElementById(`tab-${target}-content`);
        if (activeContent) {
          activeContent.classList.remove('hidden');
          activeContent.classList.add('active');
        }

        if (target === 'study') {
          loadStudyQueue();
        } else if (target === 'cards') {
          loadMyCards();
        } else if (target === 'settings') {
          loadDeckSettings();
          loadStatsChart();
        }
      });
    });
  }

  // ── 복습 큐 로드 및 렌더링 ────────────────────────────────
  async function loadStudyQueue() {
    const container = document.getElementById('study-container');
    if (!container) return;

    container.innerHTML = `<p class="loading-state">복습 큐를 불러오는 중입니다...</p>`;

    try {
      const res = await api('/api/study/queue', { method: 'GET' });
      if (!res.ok) {
        container.innerHTML = `<p class="error-msg">복습 큐를 불러오지 못했습니다.</p>`;
        return;
      }

      const data = await res.json();
      queueCards = data.cards || [];
      currentCardIndex = 0;
      isCardFlipped = false;

      updateDueHUD(data.reviews_remaining, data.new_remaining_today);

      if (queueCards.length === 0) {
        renderEmptyQueue(container, data.new_remaining_today);
      } else {
        renderCard(container);
      }
    } catch (e) {
      container.innerHTML = `<p class="error-msg">오류: ${escapeHtml(e.message)}</p>`;
    }
  }

  function renderEmptyQueue(container, newRemaining) {
    const dueElem = document.querySelector('.due');
    let hasRemainingNew = (newRemaining !== undefined && newRemaining > 0);
    if (!hasRemainingNew && dueElem) {
      const bTags = dueElem.querySelectorAll('b');
      if (bTags.length >= 2 && parseInt(bTags[1].textContent || '0', 10) > 0) {
        hasRemainingNew = true;
      }
    }

    container.innerHTML = `
      <div class="empty-study-box">
        <p class="empty-note">"현재 복습할 카드가 없습니다."</p>
        <p class="empty-subnote">
          ${hasRemainingNew ? '오늘 계획된 학습을 모두 마쳤습니다! 더 공부하고 싶으시다면 아래 버튼을 눌러보세요.' : '새 단어를 추가하거나 기본 단어 30선을 추가해보세요.'}
        </p>
        <div class="empty-actions">
          ${hasRemainingNew ? `
            <button type="button" class="action-btn-primary" id="btn-study-more-5">⚡ +5장 더 학습하기</button>
            <button type="button" class="action-btn-primary" id="btn-study-more-10">⚡ +10장 더 학습하기</button>
          ` : ''}
          <button type="button" class="action-btn-secondary" id="btn-import-seeds">
            ★ 기본 필수 다의어 30선 가져오기
          </button>
          <button type="button" class="action-btn-secondary" id="btn-go-dict">
            단어 검색하러 가기
          </button>
        </div>
      </div>
    `;

    document.getElementById('btn-study-more-5')?.addEventListener('click', () => increaseTodayGoal(5));
    document.getElementById('btn-study-more-10')?.addEventListener('click', () => increaseTodayGoal(10));
    document.getElementById('btn-import-seeds')?.addEventListener('click', importSeeds);
    document.getElementById('btn-go-dict')?.addEventListener('click', () => {
      document.querySelector('.action-tab[data-tab="dict"]')?.click();
    });
  }

  async function increaseTodayGoal(amount) {
    const container = document.getElementById('study-container');
    if (container) container.innerHTML = `<p class="loading-state">신규 단어 예산을 늘리고 복습 큐를 준비하는 중...</p>`;
    try {
      const deckId = await ensureActiveDeckId();
      const currentDeck = await getDeckDetails(deckId);
      const newLimit = (currentDeck.new_per_day || 10) + amount;

      await api('/api/decks', {
        method: 'PATCH',
        body: JSON.stringify({
          deck_id: deckId,
          new_per_day: newLimit,
        }),
      });

      await loadStudyQueue();
      await refreshDueHUD();
    } catch (e) {
      if (container) container.innerHTML = `<p class="error-msg">목표 갱신 실패: ${escapeHtml(e.message)}</p>`;
    }
  }

  function renderCard(container) {
    if (currentCardIndex >= queueCards.length) {
      renderStudyComplete(container);
      return;
    }

    const card = queueCards[currentCardIndex];
    isCardFlipped = false;

    const frontText = card.front ? (card.front.text || card.lemma) : card.lemma;
    const phonetic = card.phonetic ? `<span class="card-phonetic">${escapeHtml(card.phonetic)}</span>` : '';
    const posBadge = card.pos ? `<span class="pos-badge">${escapeHtml(card.pos)}</span>` : '';

    container.innerHTML = `
      <div class="card-viewer" data-card-id="${card.card_id}">
        <div class="card-progress">
          <span>진행: <strong>${currentCardIndex + 1}</strong> / ${queueCards.length}</span>
          ${card.is_new ? '<span class="badge-new">NEW</span>' : '<span class="badge-review">복습</span>'}
        </div>

        <div class="flashcard ${isCardFlipped ? 'flipped' : ''}" id="flashcard">
          <!-- 카드 앞면 -->
          <div class="card-face card-front">
            <div class="card-lemma-row">
              <h2 class="card-lemma">${escapeHtml(frontText)}</h2>
              <button type="button" class="btn-speaker" id="btn-speak-front" title="발음 듣기 (단축키 R)">🔊</button>
              ${phonetic}
            </div>
            <button type="button" class="flip-btn" id="flip-btn">
              <span>▶ 뜻 보기</span>
              <kbd class="key-hint">Space</kbd>
            </button>
          </div>

          <!-- 카드 뒷면 -->
          <div class="card-face card-back ${isCardFlipped ? '' : 'hidden'}">
            <div class="card-definition-row">
              ${posBadge}
              <div class="card-definition">${escapeHtml(card.definition)}</div>
            </div>
            ${card.example_en ? `
              <div class="card-example-en">
                <span>"${escapeHtml(card.example_en)}"</span>
                <button type="button" class="btn-speaker-mini" id="btn-speak-ex" title="예문 발음 듣기">🔊</button>
              </div>
            ` : ''}
            ${card.example_ko ? `<p class="card-example-ko">${escapeHtml(card.example_ko)}</p>` : ''}

            <div class="ratings" data-testid="rating-buttons">
              <button type="button" class="rating-btn" data-rating="again" data-label="Again">
                Again
                <span class="interval">지금 (1)</span>
              </button>
              <button type="button" class="rating-btn" data-rating="hard" data-label="Hard">
                Hard
                <span class="interval">내일 (2)</span>
              </button>
              <button type="button" class="rating-btn" data-rating="good" data-label="Good">
                Good
                <span class="interval">3일 뒤 (3)</span>
              </button>
              <button type="button" class="rating-btn" data-rating="easy" data-label="Easy">
                Easy
                <span class="interval">5일 뒤 (4)</span>
              </button>
            </div>
          </div>
        </div>

        <div id="card-feedback" class="card-feedback"></div>
      </div>
    `;

    // 자동 발음 재생 (약간의 딜레이)
    setTimeout(() => speakWord(card.lemma), 120);

    document.getElementById('btn-speak-front')?.addEventListener('click', () => speakWord(card.lemma));
    document.getElementById('btn-speak-ex')?.addEventListener('click', () => {
      if (card.example_en) speakWord(card.example_en);
    });
    document.getElementById('flip-btn')?.addEventListener('click', flipCard);
    container.querySelectorAll('.rating-btn').forEach((btn) => {
      btn.addEventListener('click', () => {
        submitReview(card.card_id, btn.dataset.rating);
      });
    });
  }

  function flipCard() {
    isCardFlipped = true;
    playSfx('flip');
    const front = document.querySelector('.card-front');
    const back = document.querySelector('.card-back');
    const flipBtn = document.getElementById('flip-btn');

    if (flipBtn) flipBtn.classList.add('hidden');
    if (back) back.classList.remove('hidden');
  }

  async function submitReview(cardId, rating) {
    if (rating === 'again') {
      playSfx('again');
    } else if (rating === 'hard') {
      playSfx('hard');
    } else {
      playSfx('good');
    }

    const feedback = document.getElementById('card-feedback');
    if (feedback) feedback.textContent = '평가 기록 중...';

    try {
      const res = await api('/api/study/review', {
        method: 'POST',
        body: JSON.stringify({ card_id: cardId, rating: rating }),
      });

      if (!res.ok) {
        if (feedback) feedback.textContent = '평가 저장 실패';
        return;
      }

      const outcome = await res.json();
      updateDashboardHUD(outcome);
      refreshDueHUD();

      // 다음 카드로 이동
      currentCardIndex++;
      const container = document.getElementById('study-container');
      if (container) renderCard(container);
    } catch (e) {
      if (feedback) feedback.textContent = '오류 발생';
    }
  }

  function renderStudyComplete(container) {
    playSfx('victory');
    container.innerHTML = `
      <div class="study-complete-box">
        <div class="complete-icon" aria-hidden="true">🎉</div>
        <h3 class="complete-title">✨ 오늘의 복습 미션 완료!</h3>
        <p class="complete-desc">오늘 계획된 카드를 모두 클리어했습니다! 콤보 스트릭이 이어집니다.</p>
        <div class="empty-actions">
          <button type="button" class="action-btn-primary" id="btn-study-more-5-complete">⚡ +5장 더 학습하기</button>
          <button type="button" class="action-btn-secondary" id="btn-refresh-queue">다시 확인하기</button>
        </div>
      </div>
    `;
    document.getElementById('btn-study-more-5-complete')?.addEventListener('click', () => increaseTodayGoal(5));
    document.getElementById('btn-refresh-queue')?.addEventListener('click', loadStudyQueue);
  }

  function updateDashboardHUD(outcome) {
    // HUD 수치 실시간 갱신
    if (outcome.streak) {
      const streakElem = document.querySelector('.streak');
      if (streakElem) {
        streakElem.textContent = `${outcome.streak.current}일 연속`;
      }
    }
    if (outcome.level) {
      const levelLabel = document.querySelector('.level-label');
      if (levelLabel) {
        const title = getLevelTitle(outcome.level.level);
        levelLabel.textContent = `Lv ${outcome.level.level} ${title}`;
      }

      const progress = document.querySelector('.level-progress');
      if (progress && outcome.level.xp_span > 0) {
        const percent = Math.min(100, Math.floor((outcome.level.xp_into_level * 100) / outcome.level.xp_span));
        progress.value = percent;
      }

      const xpSpan = document.querySelector('.level-xp');
      if (xpSpan) {
        xpSpan.textContent = `${outcome.level.xp_into_level} / ${outcome.level.xp_span} XP`;
      }
    }
  }

  function updateDueHUD(reviews, newRemaining) {
    const dueElem = document.querySelector('.due');
    if (dueElem) {
      const bTags = dueElem.querySelectorAll('b');
      if (bTags.length >= 2) {
        if (reviews !== undefined && reviews !== null) bTags[0].textContent = reviews;
        if (newRemaining !== undefined && newRemaining !== null) bTags[1].textContent = newRemaining;
      }
    }
  }

  async function refreshDueHUD() {
    try {
      const res = await api('/api/study/queue', { method: 'GET' });
      if (res.ok) {
        const d = await res.json();
        updateDueHUD(d.reviews_remaining, d.new_remaining_today);
      }
    } catch (_) {}
  }

  // ── 단어 사전 검색 및 추가 ─────────────────────────────────
  function setupDictSearch() {
    const form = document.getElementById('dict-search-form');
    const input = document.getElementById('dict-search-input');
    const resultBox = document.getElementById('dict-results');

    form?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const query = input?.value.trim();
      if (!query || !resultBox) return;

      resultBox.innerHTML = `<p class="loading-state">사전 조회 중: <strong>${escapeHtml(query)}</strong>...</p>`;

      try {
        const res = await api(`/api/dict/lookup?lemma=${encodeURIComponent(query)}`, { method: 'GET' });
        if (!res.ok) {
          resultBox.innerHTML = `<p class="error-msg">단어를 찾지 못했습니다.</p>`;
          return;
        }

        const data = await res.json();
        renderDictResults(data, resultBox);
      } catch (err) {
        resultBox.innerHTML = `<p class="error-msg">사전 조회 중 오류가 발생했습니다.</p>`;
      }
    });
  }

  // ── 한국어 사전 뜻 조회 ─────────────────────────────────
  async function fetchKoreanMeanings(lemma) {
    try {
      const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=ko&dt=t&dt=bd&q=${encodeURIComponent(lemma)}`;
      const res = await fetch(url);
      if (!res.ok) return null;
      const data = await res.json();
      const mainMeaning = data[0]?.[0]?.[0]?.trim() || '';
      const posMap = {};
      if (Array.isArray(data[1])) {
        for (const item of data[1]) {
          const pos = (item[0] || '').toLowerCase();
          const words = item[1] || [];
          if (pos && words.length > 0) {
            posMap[pos] = words.slice(0, 4);
          }
        }
      }
      return { mainMeaning, posMap };
    } catch (_) {
      return null;
    }
  }

  // ── 텍스트 번역 ──────────────────────────────────────────
  async function translateText(text) {
    if (!text) return '';
    try {
      const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=ko&dt=t&q=${encodeURIComponent(text)}`;
      const res = await fetch(url);
      if (res.ok) {
        const data = await res.json();
        if (data?.[0]) {
          return data[0].map((s) => s[0]).join('').trim();
        }
      }
    } catch (_) {}
    return '';
  }

  // ── 미국인들이 자주 쓰는 실생활 문장 조회 ──────────────────
  async function fetchUsDailySentence(lemma, senses, mainPos) {
    const cleanLemma = lemma.trim();
    if (!cleanLemma) return null;

    // 1순위: 사전 API의 senses 중에서 lemma가 올바르게 포함된 자연스러운 실제 예문 탐색
    if (Array.isArray(senses)) {
      const wordRegex = new RegExp(`\\b${cleanLemma}\\b`, 'i');
      for (const s of senses) {
        const ex = s.example_en?.trim();
        if (!ex) continue;
        const words = ex.split(/\s+/);
        if (words.length >= 3 && words.length <= 22 && wordRegex.test(ex)) {
          const ko = await translateText(ex);
          return { sentenceEn: ex, sentenceKo: ko };
        }
      }
    }

    // 2순위: 서버 엔드포인트 (/api/dict/sentence?lemma=...)를 통해 Tatoeba 실생활 예문 조회
    try {
      const res = await api(`/api/dict/sentence?lemma=${encodeURIComponent(cleanLemma)}`);
      if (res.ok) {
        const data = await res.json();
        if (data.sentence_en) {
          let ko = data.sentence_ko;
          if (!ko) {
            ko = await translateText(data.sentence_en);
          }
          return { sentenceEn: data.sentence_en, sentenceKo: ko };
        }
      }
    } catch (_) {}

    // 3순위 (비상 fallback): 품사에 맞는 올바른 문법 구조의 자연스러운 예문 생성
    const pos = (mainPos || '').toLowerCase();
    let sentenceEn = '';
    if (pos.includes('verb')) {
      sentenceEn = `I want to ${cleanLemma} this properly in daily life.`;
    } else if (pos.includes('noun')) {
      sentenceEn = `This is a very useful ${cleanLemma} for all of us.`;
    } else if (pos.includes('adj')) {
      sentenceEn = `It is important to stay ${cleanLemma} in this situation.`;
    } else if (pos.includes('adv')) {
      sentenceEn = `She explained the process ${cleanLemma} to everyone.`;
    } else {
      sentenceEn = `Could you explain how to use "${cleanLemma}" in this sentence?`;
    }

    const sentenceKo = await translateText(sentenceEn);
    return { sentenceEn, sentenceKo };
  }

  async function renderDictResults(data, container) {
    if (!data) {
      container.innerHTML = `<p class="empty-note">검색된 뜻이 없습니다.</p>`;
      return;
    }

    if (data.status === 'unavailable') {
      container.innerHTML = `<p class="empty-note">외부 사전 서비스에 일시적으로 연결할 수 없습니다. 아래 '나만의 뜻 직접 등록하기'에서 직접 뜻을 등록해 보세요.</p>`;
      return;
    }

    const word = data.word;
    if (!word || !word.senses || word.senses.length === 0) {
      container.innerHTML = `<p class="empty-note">검색된 뜻이 없습니다. 아래 '나만의 뜻 직접 등록하기'에서 직접 등록하실 수 있습니다.</p>`;
      return;
    }

    // 한국어 뜻과 미국인 빈출 실생활 문장 비동기 병렬 조회
    const primaryPos = word.senses.find((s) => s.pos)?.pos || '';
    const [koInfo, usSentence] = await Promise.all([
      fetchKoreanMeanings(word.lemma),
      fetchUsDailySentence(word.lemma, word.senses, primaryPos),
    ]);

    // 한국어 대표 뜻 정리
    let allMeanings = [];
    if (koInfo?.mainMeaning) allMeanings.push(koInfo.mainMeaning);
    if (koInfo?.posMap) {
      for (const p in koInfo.posMap) {
        for (const m of koInfo.posMap[p]) {
          if (!allMeanings.includes(m)) allMeanings.push(m);
        }
      }
    }
    const koMeaningsText = allMeanings.slice(0, 5).join(', ') || koInfo?.mainMeaning || '한국어 뜻 조회 완료';

    // 미국인 빈출 실생활 문장 카드 HTML
    let usSentenceCardHtml = '';
    if (usSentence && usSentence.sentenceEn) {
      usSentenceCardHtml = `
        <div class="dict-us-sentence-card">
          <div class="us-sentence-badge">
            <span>🇺🇸 미국인들이 자주 쓰는 실생활 문장</span>
          </div>
          <div class="us-sentence-en">
            <span>"${escapeHtml(usSentence.sentenceEn)}"</span>
            <button type="button" class="btn-speaker-mini" id="btn-speak-us-sentence" title="문장 발음 듣기">🔊</button>
          </div>
          <div class="us-sentence-ko">"${escapeHtml(usSentence.sentenceKo || '')}"</div>
          <div>
            <button type="button" class="btn-add-us-example mini-btn" 
                    id="btn-add-us-card"
                    data-lemma="${escapeHtml(word.lemma)}"
                    data-def="${escapeHtml(allMeanings[0] || koInfo?.mainMeaning || word.lemma)}"
                    data-ex-en="${escapeHtml(usSentence.sentenceEn)}"
                    data-ex-ko="${escapeHtml(usSentence.sentenceKo || '')}">
              + 이 실생활 예문으로 덱에 추가
            </button>
          </div>
        </div>
      `;
    }

    const phonetic = word.phonetic ? `<span class="dict-phonetic">${escapeHtml(word.phonetic)}</span>` : '';

    let sensesHtml = word.senses
      .map((s, idx) => {
        const posNorm = (s.pos || '').toLowerCase();
        const posBadge = s.pos ? `<span class="pos-badge">${escapeHtml(s.pos)}</span>` : '';
        const matchedKo = koInfo?.posMap?.[posNorm]?.slice(0, 3).join(', ') || koInfo?.mainMeaning || '';
        const koDefDisplay = matchedKo ? `<span class="dict-ko-def">📌 ${escapeHtml(matchedKo)}</span>` : '';
        const bestDef = matchedKo ? `${matchedKo}${s.pos ? ` (${s.pos})` : ''}` : s.definition;
        const ex = s.example_en ? `<div class="dict-example">"${escapeHtml(s.example_en)}"</div>` : '';

        return `
          <div class="dict-sense-item">
            <div class="dict-sense-head">
              <span class="sense-num">#${idx + 1}</span>
              ${posBadge}
              ${koDefDisplay}
            </div>
            <div class="dict-en-def">영문 정의: ${escapeHtml(s.definition)}</div>
            ${ex}
            <div class="dict-actions-row">
              <button type="button" class="btn-translate-sense mini-btn" data-text="${escapeHtml(s.definition)}">🌐 영문 정의 번역</button>
              <button type="button" class="btn-add-sense mini-btn" 
                      data-lemma="${escapeHtml(word.lemma)}"
                      data-pos="${escapeHtml(s.pos || '')}"
                      data-def="${escapeHtml(bestDef)}"
                      data-ex="${escapeHtml(s.example_en || usSentence?.sentenceEn || '')}">
                + 덱에 추가
              </button>
            </div>
            <div class="translated-box hidden"></div>
          </div>
        `;
      })
      .join('');

    container.innerHTML = `
      <div class="dict-word-card">
        <div class="dict-word-header">
          <div class="lemma-group">
            <h3 class="dict-lemma">${escapeHtml(word.lemma)}</h3>
            <button type="button" class="btn-speaker" id="btn-speak-dict" title="단어 발음 듣기">🔊</button>
            ${phonetic}
          </div>
        </div>

        <div class="dict-korean-summary">
          <span class="ko-badge">한국어 뜻</span>
          <span class="ko-meanings">${escapeHtml(koMeaningsText)}</span>
        </div>

        ${usSentenceCardHtml}

        <div class="dict-senses-list">${sensesHtml}</div>
      </div>
    `;

    document.getElementById('btn-speak-dict')?.addEventListener('click', () => speakWord(word.lemma));
    document.getElementById('btn-speak-us-sentence')?.addEventListener('click', () => {
      if (usSentence?.sentenceEn) speakWord(usSentence.sentenceEn);
    });

    // 실생활 예문으로 덱에 추가 버튼
    document.getElementById('btn-add-us-card')?.addEventListener('click', async (e) => {
      const btn = e.currentTarget;
      const lemma = btn.dataset.lemma;
      const definition = btn.dataset.def;
      const exEn = btn.dataset.exEn;
      const exKo = btn.dataset.exKo;
      const fullEx = exKo ? `${exEn} (${exKo})` : exEn;

      btn.disabled = true;
      btn.textContent = '추가 중...';

      try {
        const sRes = await api('/api/senses', {
          method: 'POST',
          body: JSON.stringify({
            lemma,
            kind: 'word',
            definition,
            example_en: fullEx,
          }),
        });

        if (!sRes.ok) {
          btn.textContent = '실패';
          return;
        }
        const senseData = await sRes.json();
        const deckId = await ensureActiveDeckId();
        const cRes = await api('/api/decks/cards', {
          method: 'POST',
          body: JSON.stringify({
            deck_id: deckId,
            sense_ids: [senseData.sense_id],
          }),
        });

        const addedCards = await cRes.json().catch(() => []);
        if (Array.isArray(addedCards) && addedCards.length > 0) {
          btn.textContent = '✔ 실생활 예문 카드 추가됨';
          btn.classList.add('added');
        } else {
          btn.textContent = '이미 추가됨';
          btn.classList.add('already-added');
        }
        refreshDueHUD();
      } catch (_) {
        btn.textContent = '오류';
      }
    });

    // 한글 번역 버튼 이벤트 바인딩
    container.querySelectorAll('.btn-translate-sense').forEach((tBtn) => {
      tBtn.addEventListener('click', async () => {
        const enText = tBtn.dataset.text;
        const itemBox = tBtn.closest('.dict-sense-item');
        const transBox = itemBox?.querySelector('.translated-box');
        if (!transBox) return;

        if (!transBox.classList.contains('hidden')) {
          transBox.classList.add('hidden');
          tBtn.textContent = '🌐 영문 정의 번역';
          return;
        }

        tBtn.disabled = true;
        tBtn.textContent = '번역 중...';
        try {
          const koText = await translateText(enText) || '번역을 가져오지 못했습니다.';
          transBox.innerHTML = `<span><strong>한글 번역:</strong> ${escapeHtml(koText)}</span>`;
          transBox.classList.remove('hidden');
          tBtn.textContent = '🌐 번역 접기';
        } catch (_) {
          transBox.textContent = '번역 실패';
          transBox.classList.remove('hidden');
          tBtn.textContent = '🌐 영문 정의 번역';
        } finally {
          tBtn.disabled = false;
        }
      });
    });

    // 덱에 추가 버튼 이벤트
    container.querySelectorAll('.btn-add-sense').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const lemma = btn.dataset.lemma;
        const pos = btn.dataset.pos || null;
        const definition = btn.dataset.def;
        const exampleEn = btn.dataset.ex || null;

        btn.disabled = true;
        btn.textContent = '추가 중...';

        try {
          // 1. Sense 생성 (기존에 있으면 재사용됨)
          const sRes = await api('/api/senses', {
            method: 'POST',
            body: JSON.stringify({
              lemma,
              kind: 'word',
              definition,
              pos,
              example_en: exampleEn,
            }),
          });

          if (!sRes.ok) {
            btn.textContent = '실패';
            return;
          }
          const senseData = await sRes.json();

          // 2. 덱에 추가
          const deckId = await ensureActiveDeckId();
          const cRes = await api('/api/decks/cards', {
            method: 'POST',
            body: JSON.stringify({
              deck_id: deckId,
              sense_ids: [senseData.sense_id],
            }),
          });

          const addedCards = await cRes.json().catch(() => []);
          if (Array.isArray(addedCards) && addedCards.length > 0) {
            btn.textContent = '✔ 추가됨';
            btn.classList.add('added');
          } else {
            btn.textContent = '이미 추가됨';
            btn.classList.add('already-added');
          }
          refreshDueHUD();
        } catch (e) {
          btn.textContent = '오류';
        }
      });
    });
  }

  // ── 직접 나만의 뜻 추가 ────────────────────────────────────
  function setupCustomSenseForm() {
    const form = document.getElementById('custom-sense-form');
    const msg = document.getElementById('custom-sense-msg');

    form?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const lemma = document.getElementById('custom-lemma')?.value.trim();
      const pos = document.getElementById('custom-pos')?.value.trim() || null;
      const definition = document.getElementById('custom-def')?.value.trim();
      const exampleEn = document.getElementById('custom-ex')?.value.trim() || null;

      if (!lemma || !definition) return;
      showMsg(msg, '저장 중...', 'info');

      try {
        const sRes = await api('/api/senses', {
          method: 'POST',
          body: JSON.stringify({
            lemma,
            kind: 'word',
            definition,
            pos,
            example_en: exampleEn,
          }),
        });

        if (!sRes.ok) {
          showMsg(msg, '[오류] 뜻을 등록하지 못했습니다.', 'error');
          return;
        }

        const senseData = await sRes.json();
        const deckId = await ensureActiveDeckId();

        const cRes = await api('/api/decks/cards', {
          method: 'POST',
          body: JSON.stringify({
            deck_id: deckId,
            sense_ids: [senseData.sense_id],
          }),
        });

        const addedCards = await cRes.json().catch(() => []);
        if (Array.isArray(addedCards) && addedCards.length > 0) {
          showMsg(msg, `✔ "${lemma} (${definition})" 카드가 등록되었습니다!`, 'success');
        } else {
          showMsg(msg, `이미 덱에 등록되어 있는 단어(뜻)입니다.`, 'info');
        }
        form.reset();
        refreshDueHUD();
      } catch (err) {
        showMsg(msg, '[오류] 서버 통신 실패', 'error');
      }
    });
  }

  // ── 시드 단어 적재 버튼 ────────────────────────────────────
  function setupSeedButton() {
    const btn = document.getElementById('btn-import-seed-global');
    btn?.addEventListener('click', importSeeds);
  }

  async function importSeeds() {
    const feedback = document.getElementById('seed-import-feedback') || document.getElementById('card-feedback');
    if (feedback) feedback.textContent = '기본 다의어 30선을 덱에 생성하는 중...';

    try {
      const res = await api('/api/decks/seed', { method: 'POST', body: '{}' });
      if (res.ok) {
        if (feedback) feedback.textContent = '✔ 기본 단어 30선 생성 완료! 복습을 시작합니다.';
        setTimeout(() => loadStudyQueue(), 400);
      } else {
        if (feedback) feedback.textContent = '시드 덱 생성에 실패했습니다.';
      }
    } catch (e) {
      if (feedback) feedback.textContent = '오류 발생';
    }
  }

  // ── 내 단어장 뷰 ──────────────────────────────────────────
  function setupCardsSearch() {
    const searchInput = document.getElementById('cards-search-input');
    searchInput?.addEventListener('input', () => {
      const q = searchInput.value.toLowerCase().trim();
      if (!q) {
        renderUserCardsList(allUserCards);
      } else {
        const filtered = allUserCards.filter((c) => {
          return c.lemma.toLowerCase().includes(q) || c.definition.toLowerCase().includes(q);
        });
        renderUserCardsList(filtered);
      }
    });
  }

  async function loadMyCards() {
    const container = document.getElementById('cards-list-container');
    const badge = document.getElementById('cards-count-badge');
    if (!container) return;

    container.innerHTML = `<p class="loading-state">내 단어장을 불러오는 중입니다...</p>`;
    try {
      const res = await api('/api/cards', { method: 'GET' });
      if (!res.ok) {
        container.innerHTML = `<p class="error-msg">단어장을 불러오지 못했습니다.</p>`;
        return;
      }
      allUserCards = await res.json();
      if (badge) badge.textContent = `${allUserCards.length}장`;
      renderUserCardsList(allUserCards);
    } catch (e) {
      container.innerHTML = `<p class="error-msg">오류: ${escapeHtml(e.message)}</p>`;
    }
  }

  function renderUserCardsList(cards) {
    const container = document.getElementById('cards-list-container');
    if (!container) return;

    if (!cards || cards.length === 0) {
      container.innerHTML = `<p class="empty-note">등록된 카드가 없습니다. 사전에서 단어를 검색하거나 나만의 뜻을 등록해 보세요.</p>`;
      return;
    }

    const html = cards.map((c) => {
      const isNew = c.state === 'new';
      const stateBadge = isNew
        ? `<span class="badge-new">NEW</span>`
        : `<span class="badge-review">복습 (${c.reps}회 / 안정도 ${c.stability ? c.stability.toFixed(1) : '-'}일)</span>`;
      const posBadge = c.pos ? `<span class="pos-badge">${escapeHtml(c.pos)}</span>` : '';
      return `
        <div class="user-card-item" data-card-id="${c.card_id}">
          <div class="user-card-main">
            <div class="user-card-lemma-row">
              <strong class="user-card-lemma">${escapeHtml(c.lemma)}</strong>
              <button type="button" class="btn-speaker-mini" data-lemma="${escapeHtml(c.lemma)}" title="발음 듣기">🔊</button>
              ${posBadge}
              ${stateBadge}
            </div>
            <div class="user-card-def">${escapeHtml(c.definition)}</div>
            ${c.example_en ? `<div class="user-card-ex">"${escapeHtml(c.example_en)}"</div>` : ''}
          </div>
          <div class="user-card-actions">
            <button type="button" class="btn-delete-card mini-btn btn-danger" data-card-id="${c.card_id}" title="카드 삭제">🗑️ 삭제</button>
          </div>
        </div>
      `;
    }).join('');

    container.innerHTML = `<div class="cards-list">${html}</div>`;

    container.querySelectorAll('.btn-speaker-mini').forEach((b) => {
      b.addEventListener('click', () => speakWord(b.dataset.lemma));
    });

    container.querySelectorAll('.btn-delete-card').forEach((b) => {
      b.addEventListener('click', async () => {
        const cardId = b.dataset.cardId;
        if (!confirm('이 카드를 단어장에서 삭제하시겠습니까?')) return;
        b.disabled = true;
        b.textContent = '삭제 중...';
        try {
          const res = await api(`/api/cards/${cardId}`, { method: 'DELETE' });
          if (res.ok) {
            allUserCards = allUserCards.filter((c) => c.card_id !== cardId);
            renderUserCardsList(allUserCards);
            const badge = document.getElementById('cards-count-badge');
            if (badge) badge.textContent = `${allUserCards.length}장`;
            refreshDueHUD();
          } else {
            alert('삭제 실패');
            b.disabled = false;
            b.textContent = '🗑️ 삭제';
          }
        } catch (_) {
          alert('오류 발생');
          b.disabled = false;
          b.textContent = '🗑️ 삭제';
        }
      });
    });
  }

  // ── 학습 설정 & 통계 뷰 ────────────────────────────────────
  async function loadDeckSettings() {
    try {
      const deckId = await ensureActiveDeckId();
      const deck = await getDeckDetails(deckId);
      const newPerDayInput = document.getElementById('setting-new-per-day');
      const dailyGoalInput = document.getElementById('setting-daily-goal');
      if (newPerDayInput && deck.new_per_day) newPerDayInput.value = deck.new_per_day;
      if (dailyGoalInput && deck.daily_goal) dailyGoalInput.value = deck.daily_goal;
    } catch (_) {}
  }

  function setupDeckSettingsForm() {
    const form = document.getElementById('deck-settings-form');
    const msg = document.getElementById('setting-save-msg');
    form?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const newPerDay = parseInt(document.getElementById('setting-new-per-day')?.value || '20', 10);
      const dailyGoal = parseInt(document.getElementById('setting-daily-goal')?.value || '20', 10);

      showMsg(msg, '저장 중...', 'info');
      try {
        const deckId = await ensureActiveDeckId();
        const res = await api('/api/decks', {
          method: 'PATCH',
          body: JSON.stringify({
            deck_id: deckId,
            new_per_day: newPerDay,
            daily_goal: dailyGoal,
          }),
        });
        if (res.ok) {
          showMsg(msg, '✔ 학습 목표가 성공적으로 저장되었습니다!', 'success');
          refreshDueHUD();
        } else {
          showMsg(msg, '[오류] 저장 실패', 'error');
        }
      } catch (e) {
        showMsg(msg, '[오류] 서버 통신 실패', 'error');
      }
    });
  }

  async function loadStatsChart() {
    const container = document.getElementById('stats-chart-container');
    if (!container) return;

    try {
      const res = await api('/api/stats/summary', { method: 'GET' });
      if (!res.ok) {
        container.innerHTML = `<p class="empty-note">통계를 불러올 수 없습니다.</p>`;
        return;
      }
      const stats = await res.json();
      renderStatsBars(stats, container);
    } catch (_) {
      container.innerHTML = `<p class="empty-note">통계 로드 중 오류가 발생했습니다.</p>`;
    }
  }

  function renderStatsBars(stats, container) {
    if (!stats || stats.length === 0) {
      container.innerHTML = `<p class="empty-note">최근 7일간의 복습 기록이 아직 없습니다. 카드를 학습하면 여기에 기록됩니다.</p>`;
      return;
    }

    const maxCount = Math.max(...stats.map((s) => s.count), 1);
    const barsHtml = stats.slice().reverse().map((s) => {
      const heightPercent = Math.max(12, Math.round((s.count / maxCount) * 100));
      return `
        <div class="stat-bar-col">
          <span class="stat-count">${s.count}회</span>
          <div class="stat-bar-track">
            <div class="stat-bar-fill" style="height: ${heightPercent}%"></div>
          </div>
          <span class="stat-date">${s.local_date.slice(5)}</span>
        </div>
      `;
    }).join('');

    const totalReviews = stats.reduce((acc, s) => acc + s.count, 0);

    container.innerHTML = `
      <div class="stats-summary-text">
        <span>최근 7일간 총 <strong>${totalReviews}건</strong>의 복습을 수행했습니다!</span>
      </div>
      <div class="stats-bars-wrapper">${barsHtml}</div>
    `;
  }

  // ── 활성 덱 ID 가져오기/생성하기 ───────────────────────────
  async function ensureActiveDeckId() {
    if (activeDeckId) return activeDeckId;

    const res = await api('/api/decks', { method: 'GET' });
    if (res.ok) {
      const decks = await res.json();
      if (decks && decks.length > 0) {
        activeDeckId = decks[0].deck_id;
        return activeDeckId;
      }
    }

    // 덱이 하나도 없으면 자동 생성
    const createRes = await api('/api/decks', {
      method: 'POST',
      body: JSON.stringify({ name: '기본 단어장' }),
    });
    if (createRes.ok) {
      const newDeck = await createRes.json();
      activeDeckId = newDeck.deck_id;
      return activeDeckId;
    }

    throw new Error('덱을 찾거나 생성할 수 없습니다.');
  }

  async function getDeckDetails(deckId) {
    const res = await api('/api/decks', { method: 'GET' });
    if (res.ok) {
      const decks = await res.json();
      const target = decks.find((d) => d.deck_id === deckId);
      if (target) return target;
    }
    return { new_per_day: 10, daily_goal: 20 };
  }

  // ── 키보드 단축키 ─────────────────────────────────────────
  function setupKeyboardShortcuts() {
    window.addEventListener('keydown', (e) => {
      // 텍스트 입력 중일 때는 단축키를 처리하지 않음
      if (['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)) return;

      const studyContainer = document.getElementById('study-container');
      const cardViewer = studyContainer?.querySelector('.card-viewer');
      if (!cardViewer) return;

      const cardId = cardViewer.dataset.cardId;
      const currentCard = queueCards[currentCardIndex];

      // R 키: 현재 카드 발음 듣기
      if (e.key === 'r' || e.key === 'R') {
        e.preventDefault();
        if (currentCard && currentCard.lemma) {
          speakWord(currentCard.lemma);
        }
      } else if (e.code === 'Space') {
        e.preventDefault();
        if (!isCardFlipped) {
          flipCard();
        }
      } else if (isCardFlipped) {
        if (e.key === '1') {
          e.preventDefault();
          submitReview(cardId, 'again');
        } else if (e.key === '2') {
          e.preventDefault();
          submitReview(cardId, 'hard');
        } else if (e.key === '3') {
          e.preventDefault();
          submitReview(cardId, 'good');
        } else if (e.key === '4') {
          e.preventDefault();
          submitReview(cardId, 'easy');
        }
      }
    });
  }

  // ── 유틸리티 ──────────────────────────────────────────────
  function showMsg(el, text, type) {
    if (!el) return;
    el.textContent = text;
    el.className = `system-msg msg-${type}`;
  }

  function escapeHtml(str) {
    if (!str) return '';
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }
})();

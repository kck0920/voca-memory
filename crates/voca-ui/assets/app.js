// Voca Memory 브라우저 인터랙션 스크립트.
//
// 레트로 HUD 스타일의 대시보드, 복습 큐, 단어 검색/추가, 설정을 관리한다.

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

  // ── 초기화 ────────────────────────────────────────────────
  document.addEventListener('DOMContentLoaded', init);

  async function init() {
    setupAuthTabs();
    setupAuthForms();
    setupActionNav();
    setupDictSearch();
    setupCustomSenseForm();
    setupKeyboardShortcuts();
    setupSeedButton();

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

      if (queueCards.length === 0) {
        renderEmptyQueue(container);
      } else {
        renderCard(container);
      }
    } catch (e) {
      container.innerHTML = `<p class="error-msg">오류: ${escapeHtml(e.message)}</p>`;
    }
  }

  function renderEmptyQueue(container) {
    container.innerHTML = `
      <div class="empty-study-box">
        <p class="empty-note">"현재 복습할 카드가 없습니다."</p>
        <p class="empty-subnote">새 단어를 추가하거나 기본 단어 30선을 추가해보세요.</p>
        <div class="empty-actions">
          <button type="button" class="action-btn-primary" id="btn-import-seeds">
            ★ 기본 필수 다의어 30선 가져오기
          </button>
          <button type="button" class="action-btn-secondary" id="btn-go-dict">
            단어 검색하러 가기
          </button>
        </div>
      </div>
    `;

    document.getElementById('btn-import-seeds')?.addEventListener('click', importSeeds);
    document.getElementById('btn-go-dict')?.addEventListener('click', () => {
      document.querySelector('.action-tab[data-tab="dict"]')?.click();
    });
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

    document.getElementById('flip-btn')?.addEventListener('click', flipCard);
    container.querySelectorAll('.rating-btn').forEach((btn) => {
      btn.addEventListener('click', () => {
        submitReview(card.card_id, btn.dataset.rating);
      });
    });
  }

  function flipCard() {
    isCardFlipped = true;
    const front = document.querySelector('.card-front');
    const back = document.querySelector('.card-back');
    const flipBtn = document.getElementById('flip-btn');

    if (flipBtn) flipBtn.classList.add('hidden');
    if (back) back.classList.remove('hidden');
  }

  async function submitReview(cardId, rating) {
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

      // 다음 카드로 이동
      currentCardIndex++;
      const container = document.getElementById('study-container');
      if (container) renderCard(container);
    } catch (e) {
      if (feedback) feedback.textContent = '오류 발생';
    }
  }

  function renderStudyComplete(container) {
    container.innerHTML = `
      <div class="study-complete-box">
        <h3 class="complete-title">✨ 오늘의 복습 완료!</h3>
        <p class="complete-desc">오늘 계획된 카드를 모두 학습했습니다. 스트릭이 이어집니다!</p>
        <button type="button" class="action-btn-primary" id="btn-refresh-queue">
          다시 확인하기
        </button>
      </div>
    `;
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
      if (levelLabel) levelLabel.textContent = `Lv ${outcome.level.level}`;

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

  function renderDictResults(data, container) {
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

    const phonetic = word.phonetic ? `<span class="dict-phonetic">${escapeHtml(word.phonetic)}</span>` : '';

    let sensesHtml = word.senses
      .map((s, idx) => {
        const pos = s.pos ? `<span class="pos-badge">${escapeHtml(s.pos)}</span>` : '';
        const ex = s.example_en ? `<div class="dict-example">"${escapeHtml(s.example_en)}"</div>` : '';
        return `
          <div class="dict-sense-item">
            <div class="dict-sense-head">
              <span class="sense-num">#${idx + 1}</span>
              ${pos}
              <span class="dict-def">${escapeHtml(s.definition)}</span>
            </div>
            ${ex}
            <button type="button" class="btn-add-sense mini-btn" 
                    data-lemma="${escapeHtml(word.lemma)}"
                    data-pos="${escapeHtml(s.pos || '')}"
                    data-def="${escapeHtml(s.definition)}"
                    data-ex="${escapeHtml(s.example_en || '')}">
              + 덱에 추가
            </button>
          </div>
        `;
      })
      .join('');

    container.innerHTML = `
      <div class="dict-word-card">
        <div class="dict-word-header">
          <h3 class="dict-lemma">${escapeHtml(word.lemma)}</h3>
          ${phonetic}
        </div>
        <div class="dict-senses-list">${sensesHtml}</div>
      </div>
    `;
  }

    container.querySelectorAll('.btn-add-sense').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const lemma = btn.dataset.lemma;
        const pos = btn.dataset.pos || null;
        const definition = btn.dataset.def;
        const exampleEn = btn.dataset.ex || null;

        btn.disabled = true;
        btn.textContent = '추가 중...';

        try {
          // 1. Sense 생성
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
          await api('/api/decks/cards', {
            method: 'POST',
            body: JSON.stringify({
              deck_id: deckId,
              sense_ids: [senseData.sense_id],
            }),
          });

          btn.textContent = '✔ 추가됨';
          btn.classList.add('added');
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

        await api('/api/decks/cards', {
          method: 'POST',
          body: JSON.stringify({
            deck_id: deckId,
            sense_ids: [senseData.sense_id],
          }),
        });

        showMsg(msg, `✔ "${lemma} (${definition})" 카드가 등록되었습니다!`, 'success');
        form.reset();
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

  // ── 키보드 단축키 ─────────────────────────────────────────
  function setupKeyboardShortcuts() {
    window.addEventListener('keydown', (e) => {
      // 텍스트 입력 중일 때는 단축키를 처리하지 않음
      if (['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)) return;

      const studyContainer = document.getElementById('study-container');
      const cardViewer = studyContainer?.querySelector('.card-viewer');
      if (!cardViewer) return;

      const cardId = cardViewer.dataset.cardId;

      if (e.code === 'Space') {
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

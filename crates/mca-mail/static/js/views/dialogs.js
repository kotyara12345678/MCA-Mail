// The dialog feed — the panel's main screen.
//
// Left: the conversation list. Centre: the chronological block feed (inbound
// mail, the agent's replies, processing events). Right: the context a
// manager needs to decide. On mobile the same three panes become two screens
// (list ↔ thread) plus a slide-over for context.

import { api } from '../api.js';
import {
  blockAuthor,
  chip,
  confirmModal,
  draftStatusLabel,
  el,
  emailStatusLabel,
  emptyState,
  errorState,
  formatListTime,
  initials,
  mount,
  previewText,
  spinner,
  toast,
} from '../ui.js';

const state = {
  search: '',
  filter: 'orders', // orders | attention | all — orders first: the panel exists to process them
  threads: [],
  total: 0,
  activeId: null,
  feed: null,
  loadingList: false,
  loadingFeed: false,
  listError: null,
  feedError: null,
};

let root = null;
let refreshTimer = null;

export function deactivate() {
  if (refreshTimer) {
    clearInterval(refreshTimer);
    refreshTimer = null;
  }
}

export async function render(container, params) {
  root = container;
  state.activeId = params.id || null;

  container.className = 'dialogs-host';
  const panes = el(
    'div',
    { class: 'dialogs' },
    renderListPane(),
    renderFeedPane(),
    renderContextPane(),
  );
  mount(container, panes);
  applyMobileClass(panes);

  await loadList();
  if (state.activeId) await loadFeed(state.activeId);

  if (!refreshTimer) {
    // Quiet revalidation: the list only refreshes when the tab is visible,
    // so a forgotten tab does not hammer the API.
    refreshTimer = setInterval(() => {
      if (document.hidden || !root.isConnected) return;
      loadList({ silent: true });
    }, 45_000);
  }
}

function applyMobileClass(panes) {
  panes.classList.toggle('thread-open', Boolean(state.activeId));
}

// ---------------------------------------------------------------------------
// list pane
// ---------------------------------------------------------------------------

function renderListPane() {
  return el(
    'div',
    { class: 'list-pane' },
    el(
      'div',
      { class: 'list-controls' },
      el(
        'div',
        { class: 'row' },
        el('input', {
          class: 'input',
          type: 'search',
          placeholder: 'Поиск: тема или адрес',
          value: state.search,
          onInput: (event) => {
            state.search = event.target.value;
            scheduleSearch();
          },
        }),
        el(
          'button',
          {
            class: 'icon-btn',
            title: 'Обновить',
            onClick: () => loadList(),
          },
          '⟳',
        ),
      ),
      el(
        'div',
        { class: 'row' },
        segmented(
          [
            ['orders', 'Заказы'],
            ['attention', 'Требуют внимания'],
            ['all', 'Все'],
          ],
          state.filter,
          (value) => {
            state.filter = value;
            loadList();
          },
        ),
        el('span', { class: 'page-sub', id: 'threads-total' }, ''),
      ),
    ),
    el('div', { class: 'thread-list', id: 'thread-list' }, spinner()),
  );
}

let searchDebounce = null;

function scheduleSearch() {
  clearTimeout(searchDebounce);
  searchDebounce = setTimeout(() => loadList(), 300);
}

function segmented(options, active, onChange) {
  return el(
    'div',
    { class: 'segmented' },
    options.map(([value, label]) =>
      el(
        'button',
        {
          class: value === active ? 'active' : '',
          onClick: () => onChange(value),
        },
        label,
      ),
    ),
  );
}

async function loadList({ silent = false } = {}) {
  state.loadingList = true;
  if (!silent) paintList();
  try {
    const data = await api.threads({
      search: state.search.trim() || undefined,
      status: state.filter === 'attention' ? 'attention' : undefined,
      flagged: state.filter === 'orders' ? true : undefined,
      limit: 100,
    });
    state.threads = data.items || [];
    state.total = data.total || 0;
    state.listError = null;
  } catch (error) {
    state.listError = error;
  } finally {
    state.loadingList = false;
    paintList();
  }
}

function paintList() {
  const list = document.getElementById('thread-list');
  const total = document.getElementById('threads-total');
  if (!list) return;
  if (total) total.textContent = state.total ? `${state.total} шт.` : '';
  if (state.loadingList && state.threads.length === 0) {
    mount(list, spinner());
    return;
  }
  if (state.listError) {
    mount(list, errorState(state.listError.message, () => loadList()));
    return;
  }
  if (state.threads.length === 0) {
    mount(
      list,
      emptyState(
        'Диалогов нет',
        state.search || state.filter !== 'all'
          ? 'Ничего не найдено — измените поиск или фильтр.'
          : 'Новые письма появятся здесь после обработки.',
      ),
    );
    return;
  }
  mount(
    list,
    state.threads.map((thread) => threadItem(thread)),
  );
}

function threadItem(thread) {
  const badges = [];
  if (thread.pending_drafts > 0) {
    badges.push(chip(`${thread.pending_drafts} черн.`, 'warn'));
  }
  if (thread.flagged) badges.push(chip('заказ', 'accent'));
  if (thread.message_count > 0) {
    badges.push(chip(`${thread.message_count} пис.`, 'neutral'));
  }
  return el(
    'button',
    {
      class: `thread-item${thread.id === state.activeId ? ' active' : ''}`,
      onClick: () => openThread(thread.id),
    },
    el(
      'div',
      { class: 't-top' },
      thread.needs_attention ? el('span', { class: 'attention-dot', title: 'Требует внимания' }) : null,
      el('span', { class: 't-name' }, thread.participant_name || thread.participant_address || 'Без адресата'),
      el('span', { class: 't-time' }, formatListTime(thread.last_activity_at)),
    ),
    el('div', { class: 't-subject' }, thread.subject || 'Без темы'),
    thread.preview ? el('div', { class: 't-preview' }, previewText(thread.preview, 90)) : null,
    badges.length ? el('div', { class: 't-badges' }, badges) : null,
  );
}

// ---------------------------------------------------------------------------
// feed pane
// ---------------------------------------------------------------------------

function renderFeedPane() {
  const thread = state.feed && state.feed.thread;
  return el(
    'div',
    { class: 'feed-pane', id: 'feed-pane' },
    el(
      'div',
      { class: 'feed-head' },
      el(
        'button',
        {
          class: 'btn btn-sm back-btn',
          onClick: () => {
            location.hash = '#/dialogs';
          },
        },
        '←',
      ),
      el(
        'div',
        { class: 'f-title', id: 'feed-title' },
        thread ? thread.subject : 'Диалог',
      ),
      el(
        'button',
        {
          class: 'btn btn-sm',
          id: 'ctx-toggle',
          onClick: () => {
            const host = root.querySelector('.dialogs');
            host.classList.toggle('show-context-mobile');
          },
        },
        'Контекст',
      ),
      el(
        'button',
        {
          class: 'icon-btn',
          title: 'Обновить',
          onClick: () => state.activeId && loadFeed(state.activeId),
        },
        '⟳',
      ),
    ),
    el('div', { class: 'feed-scroll', id: 'feed-scroll' }, feedPlaceholder()),
  );
}

function feedPlaceholder() {
  if (!state.activeId) {
    return emptyState(
      'Выберите диалог',
      'Слева — список диалогов. Каждый диалог показывает переписку как одну ленту: письма, ответы агента и события обработки.',
    );
  }
  if (state.feedError) {
    return errorState(state.feedError.message, () => loadFeed(state.activeId));
  }
  return spinner();
}

async function openThread(id) {
  state.activeId = id;
  state.feed = null;
  state.feedError = null;
  history.replaceState(null, '', `#/dialogs/${id}`);
  // Repaint selection highlight without a full list fetch.
  for (const item of document.querySelectorAll('.thread-item')) {
    item.classList.remove('active');
  }
  await loadFeed(id);
  // Rebuild the panes so the mobile thread screen shows.
  const host = root.querySelector('.dialogs');
  if (host) {
    const panes = el(
      'div',
      { class: 'dialogs thread-open' },
      renderListPane(),
      renderFeedPane(),
      renderContextPane(),
    );
    mount(root, panes);
    paintList();
    paintFeed();
    paintContext();
  }
}

async function loadFeed(id) {
  state.loadingFeed = true;
  paintFeed();
  try {
    state.feed = await api.thread(id);
    state.feedError = null;
  } catch (error) {
    if (error.status === 404) {
      state.feed = null;
      state.feedError = error;
    } else {
      state.feedError = error;
    }
  } finally {
    state.loadingFeed = false;
    paintFeed();
    paintContext();
    paintFeedTitle();
  }
}

function paintFeedTitle() {
  const title = document.getElementById('feed-title');
  if (title) {
    title.textContent =
      (state.feed && state.feed.thread && state.feed.thread.subject) || 'Диалог';
  }
}

function paintFeed() {
  const scroll = document.getElementById('feed-scroll');
  if (!scroll) return;
  if (!state.activeId) {
    mount(scroll, feedPlaceholder());
    return;
  }
  if (state.loadingFeed && !state.feed) {
    mount(scroll, spinner());
    return;
  }
  if (state.feedError) {
    mount(scroll, errorState(state.feedError.message, () => loadFeed(state.activeId)));
    return;
  }
  if (!state.feed) return;

  const items = state.feed.items || [];
  const children = [];
  if (state.feed.has_more) {
    children.push(
      el(
        'button',
        {
          class: 'btn btn-sm feed-older',
          onClick: () => loadOlder(),
        },
        'Показать ранее',
      ),
    );
  }
  if (items.length === 0) {
    children.push(emptyState('Пока пусто', 'В этом диалоге ещё нет писем, ответов и событий.'));
  } else {
    children.push(items.map((item) => feedBlock(item)));
  }
  mount(scroll, children);
}

async function loadOlder() {
  const items = state.feed.items || [];
  if (items.length === 0) return;
  const cursor = items[0].at;
  const scroll = document.getElementById('feed-scroll');
  try {
    const older = await api.thread(state.activeId, {
      before: cursor,
      limit: 60,
    });
    const seen = new Set(items.map((item) => `${item.kind}:${item.id}`));
    const merged = [...(older.items || []).filter((item) => !seen.has(`${item.kind}:${item.id}`)), ...items];
    state.feed = {
      ...state.feed,
      items: merged,
      has_more: older.has_more,
    };
    paintFeed();
    if (scroll) scroll.scrollTop = 0;
  } catch (error) {
    toast(error.message, 'error');
  }
}

function feedBlock(item) {
  if (item.kind === 'event') return eventBlock(item);
  const outbound = item.kind === 'draft' || item.direction === 'outbound';
  const author = blockAuthor(item);
  const bodyNode = el('div', { class: 'block-body clamped' }, item.body || '(пусто)');
  const moreButton = el(
    'button',
    {
      class: 'block-more',
      hidden: (item.body || '').length < 700,
      onClick: () => {
        const clamped = bodyNode.classList.toggle('clamped');
        moreButton.textContent = clamped ? 'Показать полностью' : 'Свернуть';
      },
    },
    'Показать полностью',
  );

  const head = el(
    'div',
    { class: 'block-head' },
    el('div', { class: 'avatar' }, initials(author)),
    el(
      'div',
      { class: 'block-who' },
      el('div', { class: 'block-author' }, author),
      el(
        'div',
        { class: 'block-meta' },
        [
          item.kind === 'message'
            ? item.from_address
            : (item.to || []).join(', '),
          formatListTime(item.at),
        ]
          .filter(Boolean)
          .join(' · '),
      ),
    ),
    statusChipFor(item),
  );

  const actions =
    item.kind === 'draft' && item.status === 'pending_approval'
      ? el(
          'div',
          { class: 'block-actions' },
          el(
            'button',
            {
              class: 'btn btn-primary btn-sm',
              onClick: () => decideDraft(item, 'approve'),
            },
            'Отправить',
          ),
          el(
            'button',
            {
              class: 'btn btn-sm',
              onClick: () => decideDraft(item, 'reject'),
            },
            'Отклонить',
          ),
        )
      : null;

  return el(
    'div',
    { class: `block ${outbound ? 'block-outbound' : 'block-inbound'}` },
    head,
    bodyNode,
    moreButton,
    actions,
  );
}

function statusChipFor(item) {
  if (item.kind === 'event') return null;
  if (item.kind === 'draft') {
    const status = item.status;
    const kind =
      status === 'pending_approval'
        ? 'warn'
        : status === 'sent'
          ? 'ok'
          : status === 'rejected' || status === 'suppressed'
            ? 'danger'
            : 'accent';
    return chip(draftStatusLabel(status), kind);
  }
  if (item.status && item.status !== 'processed') {
    const kind = item.status === 'failed' || item.status === 'dead' ? 'danger' : 'warn';
    return chip(emailStatusLabel(item.status), kind);
  }
  return null;
}

function eventBlock(item) {
  const severity = item.severity || 'info';
  return el(
    'div',
    { class: 'event-row event-' + severity },
    el('span', { class: 'chip' }, item.code),
    el('span', {}, item.message),
    el('span', { class: 'event-time' }, formatListTime(item.at)),
  );
}

async function decideDraft(item, action) {
  const confirmText =
    action === 'approve'
      ? 'Отправить этот ответ клиенту?'
      : 'Отклонить черновик? Письмо клиенту не уйдёт.';
  const ok = await confirmModal(
    action === 'approve' ? 'Отправить ответ' : 'Отклонить черновик',
    confirmText,
    action === 'approve' ? 'Отправить' : 'Отклонить',
  );
  if (!ok) return;
  try {
    if (action === 'approve') await api.approveDraft(item.id);
    else await api.rejectDraft(item.id);
    toast(action === 'approve' ? 'Ответ отправлен' : 'Черновик отклонён', 'ok');
    await loadFeed(state.activeId);
    loadList({ silent: true });
  } catch (error) {
    toast(error.message, 'error');
  }
}

// ---------------------------------------------------------------------------
// context pane
// ---------------------------------------------------------------------------

function renderContextPane() {
  return el('div', { class: 'context-pane', id: 'context-pane' });
}

function paintContext() {
  const pane = document.getElementById('context-pane');
  if (!pane) return;
  const feed = state.feed;
  if (!feed || !feed.thread) {
    mount(
      pane,
      el('div', { class: 'ctx-section' }, el('div', { class: 'ctx-label' }, 'Контекст')),
      el('div', { class: 'ctx-muted' }, 'Выберите диалог, чтобы увидеть карточку лида и сводку переписки.'),
    );
    return;
  }
  const thread = feed.thread;
  const lead = feed.lead;
  const sections = [];

  sections.push(
    el(
      'div',
      { class: 'ctx-section' },
      el('div', { class: 'ctx-label' }, 'Диалог'),
      el('div', { class: 'ctx-value' }, thread.subject || '—'),
      el(
        'div',
        { class: 'ctx-muted' },
        `${thread.inbound_count} входящих · ${thread.draft_count} ответов`,
      ),
      thread.needs_attention
        ? el('div', {}, chip('требует внимания', 'warn'))
        : null,
    ),
  );

  sections.push(
    el(
      'div',
      { class: 'ctx-section' },
      el('div', { class: 'ctx-label' }, 'Участник'),
      el('div', { class: 'ctx-value' }, thread.participant_name || '—'),
      el('div', { class: 'ctx-muted', style: 'overflow-wrap:anywhere' }, thread.participant_address || '—'),
    ),
  );

  if (!lead) {
    sections.push(
      el(
        'div',
        { class: 'ctx-section' },
        el('div', { class: 'ctx-label' }, 'Лид'),
        el('div', { class: 'ctx-muted' }, 'Лид не создан — это письмо не коммерческое.'),
      ),
    );
  } else {
    sections.push(
      el(
        'div',
        { class: 'ctx-section' },
        el('div', { class: 'ctx-label' }, 'Лид'),
        el('div', { class: 'ctx-value' }, lead.company_name || lead.contact_email),
        lead.company_inn
          ? el('div', { class: 'ctx-muted' }, `ИНН ${lead.company_inn}`)
          : null,
        el(
          'div',
          { class: 'ctx-muted' },
          `статус: ${lead.status}${lead.category ? ` · ${lead.category}` : ''}`,
        ),
        lead.confidence !== null && lead.confidence !== undefined
          ? el(
              'div',
              { class: 'ctx-muted' },
              `уверенность модели: ${Math.round(lead.confidence * 100)}%`,
            )
          : null,
      ),
    );

    if (lead.summary) {
      sections.push(
        el(
          'div',
          { class: 'ctx-section' },
          el('div', { class: 'ctx-label' }, 'Резюме'),
          el('div', { class: 'ctx-value' }, lead.summary),
        ),
      );
    }

    if (Array.isArray(lead.open_questions) && lead.open_questions.length > 0) {
      sections.push(
        el(
          'div',
          { class: 'ctx-section' },
          el('div', { class: 'ctx-label' }, 'Открытые вопросы'),
          el(
            'ul',
            { class: 'ctx-questions' },
            lead.open_questions.map((question) => el('li', {}, question)),
          ),
        ),
      );
    }

    const flags = [];
    if (lead.callback_requested) flags.push(chip('просит звонок', 'accent'));
    if (lead.automation_locked) flags.push(chip('автоматика выключена', 'danger'));
    if (flags.length) {
      sections.push(el('div', { class: 'ctx-section' }, flags));
    }
  }

  mount(pane, sections);
}

// UI primitives: a DOM builder, formatters, labels and small widgets.
//
// The panel never interpolates untrusted data into HTML strings. `el()` builds
// real DOM nodes and text goes through `textContent`, so a subject line or an
// email body containing markup is displayed, never executed.

/** Create an element. Children may be nodes, strings, arrays or null. */
export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === undefined || value === null || value === false) continue;
    if (key === 'class') node.className = value;
    else if (key === 'dataset') Object.assign(node.dataset, value);
    else if (key.startsWith('on') && typeof value === 'function') {
      node.addEventListener(key.slice(2).toLowerCase(), value);
    } else if (value === true) node.setAttribute(key, '');
    else node.setAttribute(key, String(value));
  }
  append(node, children);
  return node;
}

function append(node, children) {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    if (Array.isArray(child)) append(node, child);
    else if (child instanceof Node) node.appendChild(child);
    else node.appendChild(document.createTextNode(String(child)));
  }
}

/** Clear a container and put new content in its place. */
export function mount(container, ...children) {
  container.replaceChildren();
  append(container, children);
  return container;
}

// ---------------------------------------------------------------------------
// formatters (pure — covered by node tests)
// ---------------------------------------------------------------------------

const MONTHS = ['янв', 'фев', 'мар', 'апр', 'мая', 'июн', 'июл', 'авг', 'сен', 'окт', 'ноя', 'дек'];

function pad2(value) {
  return String(value).padStart(2, '0');
}

/** `2026-10-22 14:05` in the viewer's local timezone; "" for a bad input. */
export function formatDateTime(raw) {
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) return '';
  return (
    `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}` +
    ` ${pad2(date.getHours())}:${pad2(date.getMinutes())}`
  );
}

/** Short list-column time: time for today, `дд мес` for this year, else date. */
export function formatListTime(raw, now = new Date()) {
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) return '';
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  if (sameDay) return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  if (date.getFullYear() === now.getFullYear()) {
    return `${date.getDate()} ${MONTHS[date.getMonth()]}`;
  }
  return `${pad2(date.getDate())}.${pad2(date.getMonth() + 1)}.${date.getFullYear()}`;
}

/** One-line preview: whitespace collapsed, hard-capped. */
export function previewText(body, max = 160) {
  const single = String(body || '').split(/\s+/).filter(Boolean).join(' ');
  if (single.length <= max) return single;
  return `${single.slice(0, max - 1)}…`;
}

const DRAFT_LABELS = {
  pending_approval: 'ждёт решения',
  approved: 'утверждён',
  rejected: 'отклонён',
  sent: 'отправлен',
  suppressed: 'подавлен',
};

export function draftStatusLabel(status) {
  return DRAFT_LABELS[status] || status || '—';
}

const EMAIL_STATUS_LABELS = {
  pending: 'в очереди',
  processing: 'обрабатывается',
  processed: 'обработано',
  quarantined: 'карантин',
  needs_review: 'нужна проверка',
  failed: 'ошибка',
  dead: 'отброшено',
};

export function emailStatusLabel(status) {
  return EMAIL_STATUS_LABELS[status] || status || '—';
}

const MODE_LABELS = {
  dry_run: 'Наблюдение',
  review: 'Черновики',
  auto: 'Автоотправка',
};

export function modeLabel(mode) {
  return MODE_LABELS[mode] || mode || '—';
}

const MODE_DESCRIPTIONS = {
  dry_run:
    'Агент только анализирует письма. Ничего не отправляется наружу и ящик не меняется.',
  review:
    'Агент готовит ответы как черновики. Отправка — только после вашего решения.',
  auto:
    'Агент отправляет ответы автоматически в пределах лимитов и правил безопасности.',
};

export function modeDescription(mode) {
  return MODE_DESCRIPTIONS[mode] || '';
}

/** Who wrote the block, as the feed shows it. */
export function blockAuthor(item) {
  if (item.kind === 'draft') return 'AI-агент';
  if (item.kind === 'event') return item.stage || 'система';
  if (item.kind === 'message') {
    if (item.direction === 'outbound') return 'MCA';
    const name = (item.from_name || '').trim();
    return name || item.from_address || 'отправитель';
  }
  return '';
}

/** Initials for the avatar circle; falls back to "@" for bare addresses. */
export function initials(author) {
  const words = String(author || '')
    .split(/[\s@._-]+/)
    .filter(Boolean);
  if (words.length === 0) return '@';
  const first = words[0][0] || '';
  const second = words[1] ? words[1][0] : '';
  return (first + second).toUpperCase();
}

export function formatCostMicros(micros) {
  const usd = Number(micros || 0) / 1_000_000;
  if (usd === 0) return '$0';
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  return `$${usd.toFixed(2)}`;
}

export function formatCount(value) {
  return new Intl.NumberFormat('ru-RU').format(Number(value || 0));
}

// ---------------------------------------------------------------------------
// widgets
// ---------------------------------------------------------------------------

export function toast(message, kind = 'info', ttlMs = 4000) {
  const root = document.getElementById('toasts');
  if (!root) return;
  const item = el('div', { class: `toast toast-${kind}`, role: 'status' }, message);
  root.appendChild(item);
  setTimeout(() => item.remove(), ttlMs);
}

export function spinner() {
  return el('div', { class: 'spinner', role: 'status', 'aria-label': 'Загрузка' });
}

export function emptyState(title, hint) {
  return el(
    'div',
    { class: 'empty' },
    el('div', { class: 'empty-title' }, title),
    hint ? el('div', { class: 'empty-hint' }, hint) : null,
  );
}

export function errorState(message, onRetry) {
  return el(
    'div',
    { class: 'empty' },
    el('div', { class: 'empty-title' }, 'Не получилось загрузить'),
    el('div', { class: 'empty-hint' }, message),
    onRetry
      ? el('button', { class: 'btn', onClick: onRetry }, 'Повторить')
      : null,
  );
}

export function chip(text, kind = 'neutral') {
  return el('span', { class: `chip chip-${kind}` }, text);
}

/** Modal dialog; returns a `close` function. Content is real DOM. */
export function modal({ title, body, actions }) {
  const root = document.getElementById('modal-root');
  const close = () => {
    root.replaceChildren();
    document.removeEventListener('keydown', onKey);
  };
  const onKey = (event) => {
    if (event.key === 'Escape') close();
  };
  document.addEventListener('keydown', onKey);
  const dialog = el(
    'div',
    { class: 'modal', role: 'dialog', 'aria-modal': 'true' },
    el(
      'div',
      { class: 'modal-head' },
      el('div', { class: 'modal-title' }, title),
      el('button', { class: 'icon-btn', 'aria-label': 'Закрыть', onClick: close }, '×'),
    ),
    el('div', { class: 'modal-body' }, body),
    actions ? el('div', { class: 'modal-actions' }, actions) : null,
  );
  root.replaceChildren(
    el(
      'div',
      {
        class: 'modal-backdrop',
        // Close on the backdrop itself only: a click on a field or select
        // inside the dialog bubbles here and must not dismiss the window.
        onClick: (event) => {
          if (!dialog.contains(event.target)) close();
        },
      },
      dialog,
    ),
  );
  return close;
}

/** Confirm dialog built on `modal`; resolves true when confirmed. */
export function confirmModal(title, message, confirmLabel = 'Подтвердить') {
  return new Promise((resolve) => {
    const close = modal({
      title,
      body: el('div', { class: 'confirm-text' }, message),
      actions: [
        el(
          'button',
          {
            class: 'btn',
            onClick: () => {
              close();
              resolve(false);
            },
          },
          'Отмена',
        ),
        el(
          'button',
          {
            class: 'btn btn-primary',
            onClick: () => {
              close();
              resolve(true);
            },
          },
          confirmLabel,
        ),
      ],
    });
  });
}

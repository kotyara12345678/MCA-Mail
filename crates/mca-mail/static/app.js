// Panel bootstrap: session gate, shell chrome and the hash router.
//
// The whole panel is cookie-authenticated: `GET /api/auth/session` decides
// whether the login form or the shell renders, and every later request
// carries the HttpOnly session cookie (fetch `credentials: 'same-origin'`).

import { api, ApiError } from './js/api.js';
import { el, mount, toast } from './js/ui.js';
import * as dialogs from './js/views/dialogs.js';
import * as settings from './js/views/settings.js';
import * as models from './js/views/models.js';
import * as costs from './js/views/costs.js';

const ROUTES = [
  { pattern: /^\/dialogs\/([0-9a-fA-F-]+)$/, title: 'Диалоги', view: dialogs, nav: 'dialogs' },
  { pattern: /^\/dialogs$/, title: 'Диалоги', view: dialogs, nav: 'dialogs' },
  { pattern: /^\/models$/, title: 'Модели', view: models, nav: 'models' },
  { pattern: /^\/settings$/, title: 'Настройки', view: settings, nav: 'settings' },
  { pattern: /^\/costs$/, title: 'Затраты', view: costs, nav: 'costs' },
];

const appState = {
  session: null,
  attentionCount: 0,
};

const root = document.getElementById('app');

// ---------------------------------------------------------------------------
// boot
// ---------------------------------------------------------------------------

async function boot() {
  try {
    appState.session = await api.session();
  } catch {
    appState.session = { authenticated: false };
  }
  if (!appState.session.authenticated) {
    renderLogin();
    return;
  }
  renderShell();
  window.addEventListener('hashchange', onRoute);
  await onRoute();
  refreshAttentionCount();
  setInterval(() => {
    if (!document.hidden) refreshAttentionCount();
  }, 60_000);
}

async function refreshAttentionCount() {
  try {
    const data = await api.threads({ status: 'attention', limit: 1 });
    appState.attentionCount = data.total || 0;
  } catch {
    appState.attentionCount = 0;
  }
  paintNavBadges();
}

// ---------------------------------------------------------------------------
// login
// ---------------------------------------------------------------------------

function renderLogin() {
  const loginInput = el('input', { class: 'input', autocomplete: 'username', required: true });
  const passwordInput = el('input', {
    class: 'input',
    type: 'password',
    autocomplete: 'current-password',
    required: true,
  });
  const errorLine = el('div', { class: 'login-error', hidden: true });

  const form = el(
    'form',
    {
      class: 'login-card',
      onSubmit: async (event) => {
        event.preventDefault();
        errorLine.hidden = true;
        const button = form.querySelector('button[type=submit]');
        button.disabled = true;
        try {
          appState.session = await api.login(loginInput.value.trim(), passwordInput.value);
          renderShell();
          window.addEventListener('hashchange', onRoute);
          await onRoute();
          refreshAttentionCount();
        } catch (error) {
          errorLine.textContent =
            error instanceof ApiError ? error.message : 'Сервер недоступен';
          errorLine.hidden = false;
        } finally {
          button.disabled = false;
        }
      },
    },
    el('div', { class: 'brand' }, el('div', { class: 'brand-mark' }, 'M'), 'MCA Mail'),
    el('h1', { class: 'login-title' }, 'Вход в панель'),
    el('div', { class: 'field' }, el('label', {}, 'Логин'), loginInput),
    el('div', { class: 'field' }, el('label', {}, 'Пароль'), passwordInput),
    errorLine,
    el('button', { class: 'btn btn-primary', type: 'submit' }, 'Войти'),
    el(
      'div',
      { class: 'login-hint' },
      'Сессия живёт 30 дней в этом браузере. Пароль администратора задаётся переменными ADMIN_LOGIN / ADMIN_PASSWORD.',
    ),
  );

  mount(root, el('div', { class: 'login-wrap' }, form));
  loginInput.focus();
}

// ---------------------------------------------------------------------------
// shell
// ---------------------------------------------------------------------------

const NAV = [
  { id: 'dialogs', href: '#/dialogs', label: 'Диалоги', icon: '✉' },
  { id: 'models', href: '#/models', label: 'Модели', icon: '◐' },
  { id: 'settings', href: '#/settings', label: 'Настройки', icon: '⚙' },
  { id: 'costs', href: '#/costs', label: 'Затраты', icon: '₸' },
];

function renderShell() {
  const shell = el(
    'div',
    { class: 'shell' },
    el(
      'aside',
      { class: 'sidebar' },
      el('div', { class: 'brand' }, el('div', { class: 'brand-mark' }, 'M'), 'MCA Mail'),
      NAV.map((item) =>
        el(
          'a',
          {
            class: 'nav-link',
            href: item.href,
            dataset: { nav: item.id },
          },
          el('span', { 'aria-hidden': 'true' }, item.icon),
          el('span', {}, item.label),
          item.id === 'dialogs'
            ? el('span', { class: 'nav-badge', dataset: { badge: 'attention' }, hidden: true }, '0')
            : null,
        ),
      ),
      el(
        'div',
        { class: 'sidebar-foot' },
        el('div', {}, appState.session.login || ''),
        el(
          'button',
          {
            class: 'btn btn-sm',
            onClick: async () => {
              try {
                await api.logout();
              } catch {
                /* logging out locally is enough */
              }
              appState.session = { authenticated: false };
              window.removeEventListener('hashchange', onRoute);
              renderLogin();
            },
          },
          'Выйти',
        ),
      ),
    ),
    el('main', { class: 'main' }, el('div', { id: 'view' })),
  );
  mount(root, shell);
}

function paintNavBadges() {
  const badge = document.querySelector('[data-badge=attention]');
  if (!badge) return;
  badge.textContent = String(appState.attentionCount);
  badge.hidden = appState.attentionCount === 0;
}

function markActiveNav(navId) {
  for (const link of document.querySelectorAll('[data-nav]')) {
    link.classList.toggle('active', link.dataset.nav === navId);
  }
}

// ---------------------------------------------------------------------------
// routing
// ---------------------------------------------------------------------------

function currentPath() {
  const hash = location.hash || '#/dialogs';
  return hash.replace(/^#/, '') || '/dialogs';
}

async function onRoute() {
  const path = currentPath();
  const view = ROUTES.find((route) => route.pattern.test(path));
  const container = document.getElementById('view');
  if (!container) return;

  // Leaving the dialogs view stops its refresh timer.
  if (dialogs.deactivate && (!view || view.view !== dialogs)) {
    dialogs.deactivate();
  }

  markActiveNav(view ? view.nav : 'dialogs');

  if (!view) {
    location.hash = '#/dialogs';
    return;
  }

  const match = path.match(view.pattern);
  const params = {};
  if (view.view === dialogs && match && match[1]) params.id = match[1];

  container.replaceChildren();
  try {
    await view.view.render(container, params);
  } catch (error) {
    if (error instanceof ApiError && error.status === 401) {
      appState.session = { authenticated: false };
      renderLogin();
      return;
    }
    toast(error.message || 'Ошибка', 'error');
  }
}

boot();

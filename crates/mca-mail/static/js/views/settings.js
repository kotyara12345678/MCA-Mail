// Agent settings: autonomy mode, pipeline switches and service actions.
//
// The mode cards are the prominent control — they change what the agent is
// allowed to do, and the backend enforces the choice at send time, not only
// here in the UI.

import { api } from '../api.js';
import {
  el,
  emptyState,
  errorState,
  modeDescription,
  modeLabel,
  mount,
  spinner,
  toast,
} from '../ui.js';

const MODES = ['dry_run', 'review', 'auto'];

const state = {
  flags: null,
  error: null,
  busy: false,
};

export async function render(container) {
  mount(container, spinner());
  await load();
  paint(container);
}

async function load() {
  try {
    state.flags = await api.settings();
    state.error = null;
  } catch (error) {
    state.error = error;
  }
}

function paint(container) {
  if (state.error) {
    mount(container, errorState(state.error.message, () => render(container)));
    return;
  }
  const flags = state.flags;
  if (!flags) {
    mount(container, spinner());
    return;
  }

  const page = el(
    'div',
    { class: 'page-body' },
    el(
      'div',
      { class: 'card' },
      el('h2', { class: 'card-title' }, 'Режим AI-агента'),
      el(
        'div',
        { class: 'page-sub' },
        'Режим применяется на сервере сразу после переключения — без перезапуска. Отправка в режимах «Наблюдение» и «Черновики» запрещена даже для уже поставленных в очередь писем.',
      ),
      el(
        'div',
        { class: 'mode-grid' },
        MODES.map((mode) =>
          el(
            'button',
            {
              class: `mode-card${flags.email_mode === mode ? ' active' : ''}`,
              disabled: state.busy || flags.email_mode === mode,
              onClick: () => setMode(mode, container),
            },
            el('div', { class: 'm-name' }, modeLabel(mode)),
            el('div', { class: 'm-desc' }, modeDescription(mode)),
          ),
        ),
      ),
    ),
    el(
      'div',
      { class: 'card' },
      el('h2', { class: 'card-title' }, 'Пайплайн'),
      switchRow(
        'Общая автоматика',
        'Главный рубильник: выключено — агент ничего не делает, даже в режиме авто.',
        flags.automation_enabled,
        (on) => patch({ automation_enabled: on }, container),
      ),
      switchRow(
        'Загрузка писем',
        'Пауза почтового сканера.',
        !flags.poll_paused,
        (on) => patch({ poll_paused: !on }, container),
      ),
      switchRow(
        'Обработка очереди',
        'Пауза конвейера обработки.',
        !flags.queue_paused,
        (on) => patch({ queue_paused: !on }, container),
      ),
      switchRow(
        'Отправка исходящих',
        'Пауза воркера отправки.',
        !flags.outbox_paused,
        (on) => patch({ outbox_paused: !on }, container),
      ),
    ),
    el(
      'div',
      { class: 'card' },
      el('h2', { class: 'card-title' }, 'Сервисные действия'),
      el(
        'div',
        { class: 'page-sub' },
        'Ручные операции для администратора. Все действия пишутся в журнал.',
      ),
      el(
        'div',
        { style: 'display:flex;gap:8px;flex-wrap:wrap' },
        el(
          'button',
          {
            class: 'btn',
            onClick: async () => {
              try {
                const result = await api.pollNow();
                toast(
                  result.requested
                    ? 'Чтение почты запущено'
                    : 'Почтовый сканер не запущен в этом процессе',
                  result.requested ? 'ok' : 'info',
                );
              } catch (error) {
                toast(error.message, 'error');
              }
            },
          },
          'Прочитать почту сейчас',
        ),
        el(
          'button',
          {
            class: 'btn',
            onClick: async () => {
              try {
                const result = await api.recover();
                toast(`Возвращено в работу: ${result.recovered}`, 'ok');
              } catch (error) {
                toast(error.message, 'error');
              }
            },
          },
          'Вернуть зависшие письма',
        ),
        el(
          'button',
          {
            class: 'btn btn-danger',
            onClick: async () => {
              try {
                const result = await api.purgeEvents(30);
                toast(`Удалено событий старше 30 дней: ${result.purged}`, 'ok');
              } catch (error) {
                toast(error.message, 'error');
              }
            },
          },
          'Очистить журнал событий',
        ),
      ),
    ),
  );

  mount(container, page);
}

function switchRow(title, hint, on, onChange) {
  const button = el('button', {
    class: `switch${on ? ' on' : ''}`,
    role: 'switch',
    'aria-checked': String(on),
    'aria-label': title,
    disabled: state.busy,
    onClick: () => {
      const next = !button.classList.contains('on');
      button.classList.toggle('on', next);
      button.setAttribute('aria-checked', String(next));
      Promise.resolve(onChange(next)).catch(() => {
        button.classList.toggle('on', !next);
        button.setAttribute('aria-checked', String(!next));
      });
    },
  });
  return el(
    'div',
    { class: 'switch-row' },
    el(
      'div',
      {},
      el('div', { class: 'switch-label' }, title),
      el('div', { class: 'switch-hint' }, hint),
    ),
    button,
  );
}

async function setMode(mode, container) {
  state.busy = true;
  paint(container);
  try {
    state.flags = await api.patchSettings({ email_mode: mode });
    toast(`Режим агента: ${modeLabel(mode)}`, 'ok');
  } catch (error) {
    toast(error.message, 'error');
    await load();
  } finally {
    state.busy = false;
    paint(container);
  }
}

async function patch(body, container) {
  try {
    state.flags = await api.patchSettings(body);
    toast('Сохранено', 'ok', 2000);
  } catch (error) {
    toast(error.message, 'error');
    await load();
    paint(container);
    throw error;
  }
}

export function fallback() {
  return emptyState('Настройки недоступны', 'Не удалось прочитать состояние сервера.');
}

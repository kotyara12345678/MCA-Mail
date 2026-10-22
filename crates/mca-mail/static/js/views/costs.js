// Cost page: what the agent spent, by day, model and agent kind.
//
// Money is stored as integer micro-USD on the server; every conversion to a
// dollar string happens once, here, through `formatCostMicros`.

import { api } from '../api.js';
import { el, emptyState, errorState, formatCostMicros, formatCount, mount, spinner } from '../ui.js';

const state = {
  days: 30,
  usage: null,
  error: null,
};

export async function render(container) {
  mount(container, spinner());
  try {
    state.usage = await api.usage(state.days);
    state.error = null;
  } catch (error) {
    state.error = error;
  }
  paint(container);
}

function paint(container) {
  if (state.error) {
    mount(container, errorState(state.error.message, () => render(container)));
    return;
  }
  const usage = state.usage;
  if (!usage) {
    mount(container, spinner());
    return;
  }

  const daysSelect = el(
    'select',
    { class: 'select', style: 'width:auto' },
    [7, 30, 90].map((days) =>
      el('option', { value: String(days), selected: days === state.days ? '' : undefined }, `${days} дней`),
    ),
  );
  daysSelect.addEventListener('change', () => {
    state.days = Number(daysSelect.value);
    render(container);
  });

  const page = el(
    'div',
    { class: 'page-body' },
    el(
      'div',
      { style: 'display:flex;align-items:center;gap:12px' },
      el('h2', { class: 'card-title', style: 'flex:1' }, 'Затраты'),
      daysSelect,
    ),
    el(
      'div',
      { class: 'stat-grid' },
      stat('Всего', formatCostMicros(usage.total.cost_micros_usd)),
      stat('Вызовов', formatCount(usage.total.calls)),
      stat('Токены prompt', formatCount(usage.total.prompt_tokens)),
      stat('Токены completion', formatCount(usage.total.completion_tokens)),
    ),
    el(
      'div',
      { class: 'card' },
      el('h2', { class: 'card-title' }, 'По моделям'),
      usage.by_model.length === 0
        ? emptyState('Пока пусто', 'За выбранный период вызовов не было.')
        : el(
            'table',
            { class: 'table' },
            el(
              'thead',
              {},
              el(
                'tr',
                {},
                el('th', {}, 'Модель'),
                el('th', { class: 'num' }, 'Вызовов'),
                el('th', { class: 'num' }, 'Prompt'),
                el('th', { class: 'num' }, 'Completion'),
                el('th', { class: 'num' }, 'Стоимость'),
              ),
            ),
            el(
              'tbody',
              {},
              usage.by_model.map((row) =>
                el(
                  'tr',
                  {},
                  el('td', {}, row.model),
                  el('td', { class: 'num' }, formatCount(row.calls)),
                  el('td', { class: 'num' }, formatCount(row.prompt_tokens)),
                  el('td', { class: 'num' }, formatCount(row.completion_tokens)),
                  el('td', { class: 'num' }, formatCostMicros(row.cost_micros_usd)),
                ),
              ),
            ),
          ),
    ),
    el(
      'div',
      { class: 'card' },
      el('h2', { class: 'card-title' }, 'По дням'),
      usage.by_day.length === 0
        ? emptyState('Пока пусто', 'За выбранный период вызовов не было.')
        : el(
            'table',
            { class: 'table' },
            el(
              'thead',
              {},
              el(
                'tr',
                {},
                el('th', {}, 'День'),
                el('th', { class: 'num' }, 'Вызовов'),
                el('th', { class: 'num' }, 'Токены'),
                el('th', { class: 'num' }, 'Стоимость'),
              ),
            ),
            el(
              'tbody',
              {},
              usage.by_day.map((row) =>
                el(
                  'tr',
                  {},
                  el('td', {}, row.day),
                  el('td', { class: 'num' }, formatCount(row.calls)),
                  el('td', { class: 'num' }, formatCount(row.prompt_tokens + row.completion_tokens)),
                  el('td', { class: 'num' }, formatCostMicros(row.cost_micros_usd)),
                ),
              ),
            ),
          ),
    ),
    el(
      'div',
      { class: 'card' },
      el('h2', { class: 'card-title' }, 'По агентам'),
      usage.by_agent.length === 0
        ? emptyState('Пока пусто', 'Статистика появится после первых вызовов.')
        : el(
            'table',
            { class: 'table' },
            el(
              'thead',
              {},
              el(
                'tr',
                {},
                el('th', {}, 'Агент'),
                el('th', { class: 'num' }, 'Успешно'),
                el('th', { class: 'num' }, 'Ошибок'),
                el('th', { class: 'num' }, 'Токены'),
                el('th', { class: 'num' }, 'Стоимость'),
              ),
            ),
            el(
              'tbody',
              {},
              usage.by_agent.map((row) =>
                el(
                  'tr',
                  {},
                  el('td', {}, row.agent),
                  el('td', { class: 'num' }, formatCount(row.succeeded)),
                  el('td', { class: 'num' }, formatCount(row.failed)),
                  el('td', { class: 'num' }, formatCount(row.tokens)),
                  el('td', { class: 'num' }, formatCostMicros(row.cost_micros_usd)),
                ),
              ),
            ),
          ),
    ),
  );
  mount(container, page);
}

function stat(key, value) {
  return el('div', { class: 'stat' }, el('div', { class: 'v' }, value), el('div', { class: 'k' }, key));
}

// Model profiles: which LLM the agent talks to, with keys kept server-side.
//
// The list shows a key prefix, never the key. Creating or editing a profile
// is the only place a key travels: it goes over the wire once and is stored
// hashed-adjacent (the row keeps the raw key to call the provider, but it
// never comes back in a response).

import { api } from '../api.js';
import {
  confirmModal,
  el,
  errorState,
  formatCostMicros,
  formatListTime,
  modal,
  mount,
  spinner,
  toast,
} from '../ui.js';

const state = {
  profiles: [],
  presets: [],
  error: null,
};

export async function render(container) {
  mount(container, spinner());
  try {
    const [profiles, presets] = await Promise.all([api.profiles(), api.presets()]);
    state.profiles = profiles;
    state.presets = presets;
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

  const active = state.profiles.find((profile) => profile.is_active);
  const page = el(
    'div',
    { class: 'page-body' },
    el(
      'div',
      { class: 'card' },
      el(
        'div',
        { style: 'display:flex;align-items:center;gap:12px;flex-wrap:wrap' },
        el('h2', { class: 'card-title', style: 'flex:1' }, 'Модели'),
        el(
          'button',
          {
            class: 'btn btn-primary',
            onClick: () => openEditor(null, container),
          },
          'Добавить модель',
        ),
      ),
      el(
        'div',
        { class: 'page-sub' },
        active
          ? `Сейчас работает: ${active.name} · ${active.model}`
          : 'Активной модели нет — работает настройка из окружения (LLM_MODEL).',
      ),
      state.profiles.length === 0
        ? el(
            'div',
            { class: 'empty' },
            el('div', { class: 'empty-title' }, 'Профилей нет'),
            el(
              'div',
              { class: 'empty-hint' },
              'Добавьте профиль, чтобы переключить модель без перезапуска сервера.',
            ),
          )
        : profilesTable(container),
    ),
  );
  mount(container, page);
}

function profilesTable(container) {
  return el(
    'table',
    { class: 'table' },
    el(
      'thead',
      {},
      el(
        'tr',
        {},
        el('th', {}, 'Название'),
        el('th', {}, 'Модель'),
        el('th', {}, 'Цена за 1М токенов'),
        el('th', {}, 'Ключ'),
        el('th', {}, 'Обновлён'),
        el('th', {}, ''),
      ),
    ),
    el(
      'tbody',
      {},
      state.profiles.map((profile) =>
        el(
          'tr',
          {},
          el(
            'td',
            {},
            el('strong', {}, profile.name),
            profile.is_active ? el('span', { style: 'margin-left:8px' }, badge('активна')) : null,
          ),
          el('td', {}, profile.model),
          el(
            'td',
            { class: 'num' },
            `${formatCostMicros(profile.price_prompt_per_million_usd * 1_000_000)} / ${formatCostMicros(profile.price_completion_per_million_usd * 1_000_000)}`,
          ),
          el('td', { class: 'num', style: 'font-family:var(--mono)' }, profile.key_prefix || '—'),
          el('td', {}, formatListTime(profile.updated_at)),
          el(
            'td',
            { style: 'white-space:nowrap' },
            profile.is_active
              ? null
              : el(
                  'button',
                  {
                    class: 'btn btn-sm',
                    style: 'margin-right:6px',
                    onClick: () => activate(profile, container),
                  },
                  'Включить',
                ),
            el(
              'button',
              {
                class: 'btn btn-sm',
                style: 'margin-right:6px',
                onClick: () => openEditor(profile, container),
              },
              'Изменить',
            ),
            el(
              'button',
              {
                class: 'btn btn-sm btn-danger',
                onClick: () => remove(profile, container),
              },
              'Удалить',
            ),
          ),
        ),
      ),
    ),
  );
}

function badge(text) {
  return el('span', { class: 'chip chip-accent' }, text);
}

async function activate(profile, container) {
  try {
    await api.activateProfile(profile.id);
    toast(`Активна модель ${profile.name}`, 'ok');
    await render(container);
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function remove(profile, container) {
  const ok = await confirmModal(
    'Удалить профиль',
    `Удалить профиль «${profile.name}»?`,
    'Удалить',
  );
  if (!ok) return;
  try {
    await api.deleteProfile(profile.id);
    toast('Профиль удалён', 'ok');
    await render(container);
  } catch (error) {
    toast(error.message, 'error');
  }
}

function openEditor(profile, container) {
  const isNew = !profile;
  const fields = {
    name: el('input', { class: 'input', value: profile ? profile.name : '' }),
    base_url: el('input', {
      class: 'input',
      value: profile ? profile.base_url : '',
      placeholder: 'https://api.deepseek.com',
    }),
    model: el('input', {
      class: 'input',
      value: profile ? profile.model : '',
      placeholder: 'deepseek-chat',
    }),
    api_key: el('input', {
      class: 'input',
      type: 'password',
      autocomplete: 'new-password',
      placeholder: profile ? 'оставьте пустым, чтобы не менять' : 'sk-…',
    }),
    price_prompt_per_million_usd: el('input', {
      class: 'input',
      type: 'number',
      step: '0.01',
      min: '0',
      value: profile ? String(profile.price_prompt_per_million_usd) : '0.27',
    }),
    price_completion_per_million_usd: el('input', {
      class: 'input',
      type: 'number',
      step: '0.01',
      min: '0',
      value: profile ? String(profile.price_completion_per_million_usd) : '1.10',
    }),
  };

  const presetSelect = el(
    'select',
    { class: 'select' },
    el('option', { value: '' }, 'Выбрать пресет…'),
    state.presets.map((preset) =>
      el('option', { value: preset.label }, `${preset.label} · $${preset.price_prompt_per_million_usd}/$${preset.price_completion_per_million_usd}`),
    ),
  );
  presetSelect.addEventListener('change', () => {
    const preset = state.presets.find((item) => item.label === presetSelect.value);
    if (!preset) return;
    fields.base_url.value = preset.base_url;
    fields.model.value = preset.model;
    fields.price_prompt_per_million_usd.value = String(preset.price_prompt_per_million_usd);
    fields.price_completion_per_million_usd.value = String(preset.price_completion_per_million_usd);
  });

  const close = modal({
    title: isNew ? 'Новая модель' : `Изменить: ${profile.name}`,
    body: [
      isNew ? el('div', { class: 'field' }, el('label', {}, 'Пресет'), presetSelect) : null,
      field('Название', fields.name),
      field('Base URL', fields.base_url),
      field('Модель', fields.model),
      field(isNew ? 'API-ключ' : 'API-ключ (пусто = не менять)', fields.api_key),
      field('Цена prompt, $ за 1М', fields.price_prompt_per_million_usd),
      field('Цена completion, $ за 1М', fields.price_completion_per_million_usd),
    ],
    actions: [
      el(
        'button',
        {
          class: 'btn',
          onClick: () => close(),
        },
        'Отмена',
      ),
      el(
        'button',
        {
          class: 'btn btn-primary',
          onClick: async (event) => {
            const button = event.currentTarget;
            button.disabled = true;
            try {
              if (isNew) {
                await api.createProfile(readFields(fields, true));
                toast('Модель добавлена', 'ok');
              } else {
                await api.updateProfile(profile.id, readFields(fields, false));
                toast('Модель обновлена', 'ok');
              }
              close();
              await render(container);
            } catch (error) {
              toast(error.message, 'error');
              button.disabled = false;
            }
          },
        },
        isNew ? 'Добавить' : 'Сохранить',
      ),
    ],
  });
}

function field(label, input) {
  return el('div', { class: 'field' }, el('label', {}, label), input);
}

function readFields(fields, isNew) {
  const body = {
    name: fields.name.value.trim(),
    base_url: fields.base_url.value.trim(),
    model: fields.model.value.trim(),
    price_prompt_per_million_usd: Number(fields.price_prompt_per_million_usd.value),
    price_completion_per_million_usd: Number(fields.price_completion_per_million_usd.value),
  };
  const key = fields.api_key.value.trim();
  if (isNew || key) body.api_key = key;
  return body;
}

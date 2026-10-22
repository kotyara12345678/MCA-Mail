// Unit tests for the panel's pure helpers.
//
// Run with `node --test crates/mca-mail/static/tests/` — no framework, no
// network, no DOM: every function under test is pure by design so the feed's
// labels and formatting stay correct without a browser.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  blockAuthor,
  draftStatusLabel,
  emailStatusLabel,
  formatCostMicros,
  formatCount,
  formatDateTime,
  formatListTime,
  initials,
  modeDescription,
  modeLabel,
  previewText,
} from '../js/ui.js';

test('formatDateTime renders a stable local timestamp', () => {
  const raw = new Date(2026, 9, 22, 14, 5, 0).toISOString();
  assert.equal(formatDateTime(raw), '2026-10-22 14:05');
});

test('formatDateTime rejects garbage instead of showing NaN', () => {
  assert.equal(formatDateTime('yesterday'), '');
  assert.equal(formatDateTime(undefined), '');
});

test('formatListTime shows time for today, day-month for this year', () => {
  const now = new Date(2026, 9, 22, 15, 0, 0);
  const today = new Date(2026, 9, 22, 9, 7, 0).toISOString();
  assert.equal(formatListTime(today, now), '09:07');

  const earlier = new Date(2026, 2, 3, 12, 0, 0).toISOString();
  assert.equal(formatListTime(earlier, now), '3 мар');

  const lastYear = new Date(2025, 11, 31, 12, 0, 0).toISOString();
  assert.equal(formatListTime(lastYear, now), '31.12.2025');
});

test('previewText collapses whitespace and caps length', () => {
  assert.equal(previewText('  много   пробелов \n и строк '), 'много пробелов и строк');
  const long = 'слово '.repeat(100).trim();
  const preview = previewText(long, 20);
  assert.ok(preview.length <= 20);
  assert.ok(preview.endsWith('…'));
});

test('draft statuses map to human labels', () => {
  assert.equal(draftStatusLabel('pending_approval'), 'ждёт решения');
  assert.equal(draftStatusLabel('sent'), 'отправлен');
  assert.equal(draftStatusLabel('unknown-status'), 'unknown-status');
});

test('email statuses map to human labels', () => {
  assert.equal(emailStatusLabel('needs_review'), 'нужна проверка');
  assert.equal(emailStatusLabel('processed'), 'обработано');
  assert.equal(emailStatusLabel(''), '—');
});

test('agent modes carry label and explanation', () => {
  assert.equal(modeLabel('dry_run'), 'Наблюдение');
  assert.equal(modeLabel('auto'), 'Автоотправка');
  assert.ok(modeDescription('dry_run').includes('анализирует'));
  assert.ok(modeDescription('review').includes('черновик'));
  assert.equal(modeDescription('nonsense'), '');
});

test('blockAuthor names the right side of every block', () => {
  assert.equal(
    blockAuthor({ kind: 'draft' }),
    'AI-агент',
  );
  assert.equal(
    blockAuthor({ kind: 'event', stage: 'spam' }),
    'spam',
  );
  assert.equal(
    blockAuthor({ kind: 'event' }),
    'система',
  );
  assert.equal(
    blockAuthor({ kind: 'message', direction: 'inbound', from_name: 'Иван', from_address: 'a@b.c' }),
    'Иван',
  );
  assert.equal(
    blockAuthor({ kind: 'message', direction: 'inbound', from_name: null, from_address: 'a@b.c' }),
    'a@b.c',
  );
  assert.equal(
    blockAuthor({ kind: 'message', direction: 'outbound' }),
    'MCA',
  );
});

test('initials reduce an author to one or two letters', () => {
  assert.equal(initials('Иван Петров'), 'ИП');
  assert.equal(initials('client@example.com'), 'CE');
  assert.equal(initials(''), '@');
});

test('cost micros render as compact dollars', () => {
  assert.equal(formatCostMicros(0), '$0');
  assert.equal(formatCostMicros(2_500_000), '$2.50');
  assert.equal(formatCostMicros(1_234), '$0.0012');
});

test('counts use grouped digits', () => {
  assert.equal(formatCount(1234567), '1 234 567');
  assert.equal(formatCount(0), '0');
});

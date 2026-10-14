/* Панель MCA Mail: hash-роутинг на чистом JS, без сборки.
   Правило файла: всё, что пришло с сервера, проходит через esc().
   Разделы: Обзор, Письма, Черновики, Лиды, Модели, Затраты, Журнал. */

"use strict";

/* ---------- мелкие помощники ---------- */

const $ = (sel) => document.querySelector(sel);

function esc(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function fmtNum(n) {
  if (n === null || n === undefined) return "—";
  return Number(n).toLocaleString("ru-RU");
}

/* Цены хранятся в микро-долларах (1e-6 $). */
function fmtMoney(micros) {
  if (micros === null || micros === undefined) return "—";
  const usd = Number(micros) / 1e6;
  if (usd === 0) return "$0.00";
  if (usd < 0.01) return "$" + usd.toFixed(4);
  return "$" + usd.toFixed(2);
}

function fmtDate(iso) {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return esc(iso);
  return d.toLocaleString("ru-RU", {
    day: "2-digit", month: "2-digit", year: "numeric",
    hour: "2-digit", minute: "2-digit",
  });
}

function fmtDuration(totalSeconds) {
  const s = Math.max(0, Math.floor(totalSeconds || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return h + " ч " + m + " мин";
  if (m > 0) return m + " мин";
  return s + " с";
}

function icon(name) {
  return '<svg class="ic" aria-hidden="true"><use href="#i-' + name + '"/></svg>';
}

function emptyState(text, iconName) {
  return '<div class="card empty">' + icon(iconName || "inbox") +
    "<p>" + esc(text) + "</p></div>";
}

/* Ячейка с подписью: на телефоне подпись встаёт слева от значения. */
function td(label, html, cls) {
  return "<td data-label=\"" + esc(label) + "\"" +
    (cls ? " class=\"" + cls + "\"" : "") + ">" + html + "</td>";
}

function table(headers, rows, emptyText, iconName) {
  if (!rows.length) return emptyState(emptyText, iconName);
  const head = headers.map((h) =>
    "<th" + (h.num ? " class=\"num\"" : "") + ">" + esc(h.t) + "</th>").join("");
  return '<div class="table-wrap"><table><thead><tr>' + head +
    "</tr></thead><tbody>" + rows + "</tbody></table></div>";
}

function head(title, sub) {
  return '<div class="page-head"><h2>' + esc(title) + "</h2>" +
    (sub ? '<div class="sub">' + esc(sub) + "</div>" : "") + "</div>";
}

let toastTimer = null;
function toast(message, isError) {
  const node = $("#toast");
  node.textContent = message;
  node.classList.toggle("err", !!isError);
  node.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => node.classList.add("hidden"), 3500);
}

/* ---------- API ---------- */

async function api(path, options) {
  const opts = Object.assign({ credentials: "same-origin" }, options || {});
  const headers = Object.assign({}, opts.headers);
  if (opts.body && typeof opts.body !== "string") {
    headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(opts.body);
  }
  opts.headers = headers;
  const res = await fetch(path, opts);
  let data = null;
  const text = await res.text();
  if (text) {
    try { data = JSON.parse(text); } catch (_) { data = null; }
  }
  if (!res.ok) {
    const message = data && data.message ? data.message : ("Ошибка " + res.status);
    const err = new Error(message);
    err.status = res.status;
    throw err;
  }
  return data;
}

/* ---------- модальное окно ---------- */

let modalOnOk = null;

function openModal({ title, body, okText, onOk }) {
  $("#modal-title").textContent = title;
  $("#modal-body").innerHTML = body;
  $("#modal-ok").textContent = okText || "Сохранить";
  $("#modal-ok").classList.remove("danger");
  modalOnOk = onOk;
  $("#modal").classList.remove("hidden");
  const first = $("#modal-body input, #modal-body select");
  if (first) first.focus();
}

function closeModal() {
  $("#modal").classList.add("hidden");
  modalOnOk = null;
}

function confirmAction(title, text, onOk) {
  openModal({
    title,
    body: '<p style="margin:0;color:var(--muted)">' + esc(text) + "</p>",
    okText: "Да, выполнить",
    onOk,
  });
}

function modalField(id, label, value, type) {
  return "<label>" + esc(label) +
    "<input id=\"m-" + id + "\" type=\"" + (type || "text") +
    "\" value=\"" + esc(value ?? "") + "\"></label>";
}

function bindModal() {
  $("#modal-cancel").addEventListener("click", closeModal);
  $("#modal").addEventListener("click", (e) => {
    if (e.target === $("#modal")) closeModal();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("#modal").classList.contains("hidden")) closeModal();
  });
  $("#modal-ok").addEventListener("click", async () => {
    if (!modalOnOk) return closeModal();
    const btn = $("#modal-ok");
    btn.disabled = true;
    try {
      await modalOnOk();
      closeModal();
    } catch (err) {
      toast(err.message, true);
    } finally {
      btn.disabled = false;
    }
  });
}

/* ---------- состояние ---------- */

const state = {
  page: "dashboard",
  emailId: null,
  emails: { offset: 0, limit: 30, status: "", category: "", flagged: "", search: "" },
  drafts: { offset: 0, limit: 20, status: "pending_approval" },
  leads: { offset: 0, limit: 30 },
  events: { offset: 0, limit: 40 },
  usageDays: 30,
  session: null,
  summary: null,
};

/* ---------- текстовые метки ---------- */

const STATUS_LABELS = {
  pending: "в очереди",
  processing: "обрабатывается",
  processed: "обработано",
  failed: "ошибка",
  dead: "отброшено",
  quarantined: "карантин",
  needs_review: "на проверке",
};

const CATEGORY_LABELS = {
  transport_request: "заявка на перевозку",
  price_request: "запрос цены",
  internal: "внутреннее",
  spam: "спам",
  other: "прочее",
  no: "—",
};

const DRAFT_LABELS = {
  pending_approval: "ждёт решения",
  approved: "одобрен",
  rejected: "отклонён",
  sent: "отправлен",
  suppressed: "остановлен",
};

const SEVERITY_LABELS = { debug: "debug", info: "info", warning: "warning", error: "error" };

function statusBadge(status) {
  const cls = status === "processed" ? "ok"
    : status === "failed" || status === "dead" ? "danger"
    : status === "needs_review" || status === "pending" ? "warn"
    : "info";
  return '<span class="badge ' + cls + '">' + esc(STATUS_LABELS[status] || status) + "</span>";
}

function draftBadge(status) {
  const cls = status === "approved" || status === "sent" ? "ok"
    : status === "rejected" ? "danger"
    : status === "pending_approval" ? "warn"
    : "info";
  return '<span class="badge ' + cls + '">' + esc(DRAFT_LABELS[status] || status) + "</span>";
}

function severityBadge(sev) {
  const cls = sev === "error" ? "danger" : sev === "warning" ? "warn" : "info";
  return '<span class="badge ' + cls + '">' + esc(SEVERITY_LABELS[sev] || sev) + "</span>";
}

/* ---------- аутентификация ---------- */

async function checkSession() {
  try {
    state.session = await api("/api/auth/session");
  } catch (_) {
    state.session = { authenticated: false };
  }
  if (state.session.authenticated) {
    showApp();
  } else {
    showLogin();
  }
}

function showLogin() {
  $("#app-view").classList.add("hidden");
  $("#login-view").classList.remove("hidden");
}

async function showApp() {
  $("#login-view").classList.add("hidden");
  $("#app-view").classList.remove("hidden");
  const login = state.session.login || "";
  $("#user-box").textContent = login ? "Вы вошли: " + login : "";
  await loadBadges();
  route();
}

/* Красная точка у «Черновиков» — сколько ждёт решения; у «Писем» — сколько
   писем ждут ручной проверки. */
async function loadBadges() {
  try {
    const s = await api("/api/admin/summary");
    state.summary = s;
    setBadge("#nav-drafts", s.drafts_pending || 0);
    setBadge("#nav-emails", (s.emails.by_status || {}).needs_review || 0);
  } catch (_) { /* счётчик — не повод ронять вход */ }
}

function setBadge(selector, count) {
  const dot = $(selector);
  if (!dot) return;
  dot.textContent = count > 99 ? "99+" : String(count);
  dot.classList.toggle("hidden", count <= 0);
}

function bindLogin() {
  $("#login-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const errorBox = $("#login-error");
    errorBox.classList.add("hidden");
    $("#login-btn").disabled = true;
    try {
      state.session = await api("/api/auth/login", {
        method: "POST",
        body: {
          login: $("#login-input").value.trim(),
          password: $("#login-password").value,
        },
      });
      $("#login-password").value = "";
      await showApp();
    } catch (err) {
      errorBox.textContent = err.message;
      errorBox.classList.remove("hidden");
    } finally {
      $("#login-btn").disabled = false;
    }
  });

  $("#logout-btn").addEventListener("click", async () => {
    try { await api("/api/auth/logout", { method: "POST" }); } catch (_) { /* уже вышли */ }
    state.session = { authenticated: false };
    location.hash = "";
    showLogin();
  });
}

/* ---------- маршрутизатор ---------- */

const PAGES = {
  dashboard: renderDashboard,
  emails: renderEmails,
  drafts: renderDrafts,
  leads: renderLeads,
  models: renderModels,
  usage: renderUsage,
  journal: renderJournal,
};

function route() {
  if (!state.session || !state.session.authenticated) return;
  const hash = location.hash.replace(/^#\/?/, "");
  const parts = hash.split("/").filter(Boolean);
  const page = parts[0] || "dashboard";

  if (page === "emails" && parts[1]) {
    state.page = "emails";
    state.emailId = parts[1];
  } else {
    state.page = PAGES[page] ? page : "dashboard";
    state.emailId = null;
  }

  document.querySelectorAll("#nav a").forEach((a) => {
    a.classList.toggle("active", a.dataset.page === state.page);
  });

  const render = PAGES[state.page];
  $("#main").innerHTML = '<div class="page-loading"><span class="spinner"></span> Загрузка…</div>';
  render().catch((err) => {
    $("#main").innerHTML =
      '<div class="page-head"><h2>Не получилось загрузить</h2></div>' +
      '<div class="card"><p class="error" style="margin:0">' + esc(err.message) + "</p>" +
      '<p style="margin:12px 0 0"><button class="btn" onclick="location.reload()">Обновить страницу</button></p></div>';
  });
}

window.addEventListener("hashchange", route);

/* Навешивает обработчик с авто-снятием блокировки и показом ошибок тостом. */
function bind(selector, handler) {
  document.querySelectorAll(selector).forEach((btn) => {
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      try { await handler(btn); } catch (err) { toast(err.message, true); }
      btn.disabled = false;
    });
  });
}

function select(id, label, current, options) {
  const opts = options.map(([value, text]) =>
    '<option value="' + esc(value) + '"' + (value === current ? " selected" : "") + ">" +
    esc(text) + "</option>").join("");
  return '<label class="small muted" style="display:flex;flex-direction:column;gap:4px">' +
    esc(label) + '<select id="' + id + '">' + opts + "</select></label>";
}

function pager(scope, offset, limit, total) {
  const prevDisabled = offset <= 0 ? " disabled" : "";
  const nextDisabled = offset + limit >= total ? " disabled" : "";
  const from = total === 0 ? 0 : offset + 1;
  const to = Math.min(offset + limit, total);
  return '<div class="pager">' +
    '<button class="btn sm" data-pager="' + scope + '" data-dir="-1"' + prevDisabled + ">← назад</button>" +
    '<button class="btn sm" data-pager="' + scope + '" data-dir="1"' + nextDisabled + ">вперёд →</button>" +
    '<span class="muted small">' + from + "–" + to + " из " + fmtNum(total) + "</span>" +
    "</div>";
}

function bindPager(scope, rerender) {
  document.querySelectorAll('[data-pager="' + scope + '"]').forEach((btn) => {
    btn.addEventListener("click", () => {
      const dir = Number(btn.dataset.dir);
      state[scope].offset = Math.max(0, state[scope].offset + dir * state[scope].limit);
      rerender();
    });
  });
}

/* ---------- Обзор ---------- */

async function renderDashboard() {
  const s = await api("/api/admin/summary");
  state.summary = s;
  const counts = s.emails.by_status || {};
  const p = s.pipeline;

  $("#main").innerHTML = [
    head("Обзор", "Что происходит с почтой прямо сейчас"),

    '<div class="cards section">',
      metric(fmtNum(s.emails.total), "писем всего"),
      metric(fmtNum(s.emails.flagged), "флаг «заказ»", s.emails.flagged > 0 ? "danger" : ""),
      metric(fmtNum(counts.needs_review || 0), "ждут проверки", "accent"),
      metric(fmtNum(counts.failed || 0) , "с ошибкой", (counts.failed || 0) > 0 ? "danger" : ""),
      metric(fmtNum(s.drafts_pending), "черновиков на решение", s.drafts_pending > 0 ? "accent" : ""),
      metric(fmtNum(s.leads_total), "лидов"),
      metric(fmtMoney(s.cost_today_micros), "затрат сегодня"),
      metric(fmtMoney(s.cost_total_micros), "затрат всего"),
    "</div>",

    '<div class="card section">',
      "<h3 style=\"margin-top:0\">Пайплайн</h3>",
      switchRow("sw-poll", "Получение почты", "IMAP-опрос: письма подтягиваются", !p.poll_paused),
      switchRow("sw-queue", "Обработка", "новые письма уходят в ИИ-конвейер", !p.queue_paused),
      switchRow("sw-out", "Отправка ответов", "исходящие уходят наружу", !p.outbox_paused),
      switchRow("sw-auto", "Автоматика", "автоответы и карточки менеджеру", p.automation_enabled),
    "</div>",

    '<div class="card section">',
      "<h3 style=\"margin-top:0\">Быстрые действия</h3>",
      '<div class="quick-actions">',
        '<button class="btn" id="qa-poll">' + icon("refresh") + "Проверить почту сейчас</button>",
        '<button class="btn" id="qa-recover">' + icon("inbox") + "Снять зависшие обработки</button>",
        '<a class="btn" href="#/drafts">' + icon("draft") + "Черновики: " + fmtNum(s.drafts_pending) + "</a>",
      "</div>",
      '<p class="muted small" style="margin:12px 0 0">«Проверить почту» сразу запрашивает новые письма, ' +
        "не дожидаясь расписания. «Снять зависшие» возвращает в очередь письма, застрявшие после сбоя.",
    "</div>",

    '<div class="card section">',
      "<h3 style=\"margin-top:0\">Модель ИИ</h3>",
      "<p style=\"margin:0\">Активна: <b>" + esc(s.llm.active_model || "—") + "</b> " +
        (s.llm.profile
          ? '<span class="badge info">профиль «' + esc(s.llm.profile.name) + "»</span>"
          : '<span class="muted small">из конфигурации</span>') + "</p>",
      '<p class="muted small" style="margin:8px 0 0">Провайдер: ' + esc(s.llm.provider) +
        ' · <a class="link" href="#/models">сменить модель</a></p>',
    "</div>",

    '<p class="muted small">Аптайм: ' + esc(fmtDuration(s.uptime_seconds)) +
      " · база данных: " + (s.db_ok ? "на связи ✓" : "недоступна ✗") + "</p>",
  ].join("");

  // Переключатели сохраняются по одному, сразу.
  bindSwitch("sw-poll", "poll_paused", !p.poll_paused);
  bindSwitch("sw-queue", "queue_paused", !p.queue_paused);
  bindSwitch("sw-out", "outbox_paused", !p.outbox_paused);
  bindSwitch("sw-auto", "automation_enabled", p.automation_enabled);

  bind("#qa-poll", async () => {
    const r = await api("/api/admin/ops/poll-now", { method: "POST" });
    toast(r.requested
      ? "Запрос на проверку почты отправлен"
      : "Почтовый цикл не запущен в этом окружении", !r.requested);
  });
  bind("#qa-recover", async () => {
    const r = await api("/api/admin/ops/recover", { method: "POST" });
    toast(r.recovered > 0
      ? "Снято с зависания: " + r.recovered
      : "Зависших писем нет");
  });
}

function metric(value, label, cls) {
  return '<div class="card metric ' + (cls || "") + '">' +
    '<div class="value">' + value + "</div>" +
    '<div class="label">' + esc(label) + "</div></div>";
}

function switchRow(id, title, desc, checked) {
  return '<div class="switch-row"><div><div class="t">' + esc(title) + "</div>" +
    '<div class="desc">' + esc(desc) + "</div></div>" +
    '<label class="switch"><input type="checkbox" id="' + id + '"' +
    (checked ? " checked" : "") + '><span class="track"></span></label></div>';
}

/* Сохраняет один флаг настройки; checked — это НЕ значение флага, а положение
   свитча (для пауз инверсия). */
function bindSwitch(id, key, checkedWhen) {
  const box = $("#" + id);
  box.checked = !!checkedWhen;
  box.addEventListener("change", async () => {
    box.disabled = true;
    try {
      // Паузы: свитч включён == работает == флаг false.
      const inverted = key.endsWith("_paused");
      const value = inverted ? !box.checked : box.checked;
      await api("/api/admin/settings", { method: "PATCH", body: { [key]: value } });
      toast("Сохранено");
    } catch (err) {
      box.checked = !box.checked;
      toast(err.message, true);
    } finally {
      box.disabled = false;
    }
  });
}

/* ---------- Письма ---------- */

function emailFiltersQuery() {
  const f = state.emails;
  const params = new URLSearchParams();
  params.set("limit", String(f.limit));
  params.set("offset", String(f.offset));
  if (f.status) params.set("status", f.status);
  if (f.category) params.set("category", f.category);
  if (f.flagged) params.set("flagged", f.flagged);
  if (f.search) params.set("search", f.search);
  return params.toString();
}

async function renderEmails() {
  if (state.emailId) return renderEmailDetail(state.emailId);
  const f = state.emails;
  const data = await api("/api/v1/emails?" + emailFiltersQuery());

  const rows = data.items.map((row) => {
    const flag = row.order_flag ? '<span class="flag" title="заказ"></span>' : "";
    return "<tr>" +
      td("Тема", flag + '<a class="link" href="#/emails/' + esc(row.id) + '">' +
        esc(row.subject || "(без темы)") + "</a>") +
      td("Отправитель", esc(row.from_address)) +
      td("Статус", statusBadge(row.status)) +
      td("Категория", esc(CATEGORY_LABELS[row.category] || row.category || "—")) +
      td("Получено", esc(fmtDate(row.received_at)), "num") +
      "</tr>";
  }).join("");

  $("#main").innerHTML = [
    head("Письма", "Все входящие: фильтры, поиск, разбор каждого письма"),
    '<div class="filter-bar">',
      select("f-status", "Статус", f.status, Object.entries({ "": "все" }).concat(
        Object.entries(STATUS_LABELS))),
      select("f-category", "Категория", f.category, Object.entries({ "": "все" }).concat(
        Object.entries(CATEGORY_LABELS).filter(([k]) => k !== "no"))),
      select("f-flagged", "Флаг «заказ»", f.flagged,
        [["", "все"], ["true", "с флагом"], ["false", "без флага"]]),
      '<input type="search" id="f-search" placeholder="поиск по теме или отправителю" value="' +
        esc(f.search) + '">',
      '<button class="btn primary" id="f-apply">Найти</button>',
    "</div>",
    table(
      [{ t: "Тема" }, { t: "Отправитель" }, { t: "Статус" }, { t: "Категория" }, { t: "Получено", num: true }],
      rows,
      "Ничего не найдено — измените фильтры",
      "mail"),
    pager("emails", f.offset, f.limit, data.total),
  ].join("");

  $("#f-apply").addEventListener("click", applyEmailFilters);
  $("#f-search").addEventListener("keydown", (e) => {
    if (e.key === "Enter") applyEmailFilters();
  });
  bindPager("emails", renderEmails);
}

function applyEmailFilters() {
  const f = state.emails;
  f.status = $("#f-status").value;
  f.category = $("#f-category").value;
  f.flagged = $("#f-flagged").value;
  f.search = $("#f-search").value.trim();
  f.offset = 0;
  renderEmails();
}

async function renderEmailDetail(id) {
  const [email, runs, events] = await Promise.all([
    api("/api/v1/emails/" + encodeURIComponent(id)),
    api("/api/admin/emails/" + encodeURIComponent(id) + "/runs?limit=50")
      .catch(() => ({ runs: [], agent_runs: [] })),
    api("/api/admin/emails/" + encodeURIComponent(id) + "/events?limit=30").catch(() => []),
  ]);

  const runRows = (runs.runs || []).map((r) =>
    "<tr>" +
    td("Состояние", esc(r.state)) +
    td("Триггер", esc(r.trigger)) +
    td("Попытка", esc(fmtNum(r.attempt)), "num") +
    td("Токены", esc(fmtNum(r.total_tokens)), "num") +
    td("Затраты", esc(fmtMoney(r.cost_micros_usd)), "num") +
    td("Начато", esc(fmtDate(r.started_at)), "num") +
    "</tr>").join("");

  const agentRows = (runs.agent_runs || []).map((r) =>
    "<tr>" +
    td("Агент", esc(r.agent)) +
    td("Модель", esc(r.model)) +
    td("Состояние", esc(r.state)) +
    td("Токены", esc(fmtNum(r.prompt_tokens + r.completion_tokens)), "num") +
    td("Затраты", esc(fmtMoney(r.cost_micros_usd)), "num") +
    td("Начато", esc(fmtDate(r.started_at)), "num") +
    "</tr>").join("");

  const eventRows = (events || []).map((e) =>
    "<tr>" +
    td("Время", esc(fmtDate(e.created_at)), "num") +
    td("Уровень", severityBadge(e.severity)) +
    td("Код", '<span class="mono small">' + esc(e.code) + "</span>") +
    td("Сообщение", esc(e.message)) +
    "</tr>").join("");

  $("#main").innerHTML = [
    '<p class="small"><a class="link" href="#/emails">← все письма</a></p>',
    '<div class="page-head"><h2>' +
      (email.order_flag ? '<span class="flag" title="заказ"></span>' : "") +
      esc(email.subject || "(без темы)") + "</h2></div>",

    '<div class="card section">',
      '<div class="kv">',
        "От: <b>" + esc(email.from_name || "") + " &lt;" + esc(email.from_address) + "&gt;</b><br>",
        "Кому: " + esc((email.to_addresses || []).join(", ")) + "<br>",
        "Получено: " + esc(fmtDate(email.received_at)) +
          (email.date ? " (дата письма: " + esc(fmtDate(email.date)) + ")" : ""),
      "</div>",
      '<p style="margin:12px 0 0">' + statusBadge(email.status) +
        ' <span class="badge">' + esc(CATEGORY_LABELS[email.category] || email.category || "—") + "</span>" +
        (email.spam_verdict ? ' <span class="badge">спам: ' + esc(email.spam_verdict) + "</span>" : "") +
        (email.order_flag ? ' <span class="badge danger">заказ</span>' : "") +
        (email.lead_id ? ' <a class="link" href="#/leads">→ лид есть</a>' : "") +
      "</p>",
      '<div class="action-row">',
        '<button class="btn" data-act="reprocess">' + icon("refresh") + "Обработать заново</button>",
        '<button class="btn" data-act="review">На проверку</button>',
        '<button class="btn primary" data-act="approve">' + icon("check") + "Одобрить</button>",
        '<button class="btn danger" data-act="reject">' + icon("x") + "Отклонить</button>",
      "</div>",
      '<div class="detail-body" style="margin-top:14px">' + esc(email.text_body || "(пусто)") + "</div>",
    "</div>",

    '<div class="section"><h3>Запуски обработки</h3>' +
      table([{ t: "Состояние" }, { t: "Триггер" }, { t: "Попытка", num: true },
        { t: "Токены", num: true }, { t: "Затраты", num: true }, { t: "Начато", num: true }],
        runRows, "запусков нет", "activity") + "</div>",

    '<div class="section"><h3>Вызовы моделей</h3>' +
      table([{ t: "Агент" }, { t: "Модель" }, { t: "Состояние" },
        { t: "Токены", num: true }, { t: "Затраты", num: true }, { t: "Начато", num: true }],
        agentRows, "вызовов не было — затраты пишутся только для новых запусков", "cpu") + "</div>",

    '<div class="section"><h3>События письма</h3>' +
      table([{ t: "Время", num: true }, { t: "Уровень" }, { t: "Код" }, { t: "Сообщение" }],
        eventRows, "событий нет", "activity") + "</div>",
  ].join("");

  document.querySelectorAll("[data-act]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      try {
        const res = await api(
          "/api/v1/emails/" + encodeURIComponent(id) + "/" + btn.dataset.act,
          { method: "POST" });
        toast(res.message || "Готово");
        await loadBadges();
        renderEmailDetail(id);
      } catch (err) {
        toast(err.message, true);
        btn.disabled = false;
      }
    });
  });
}

/* ---------- Черновики ---------- */

async function renderDrafts() {
  const f = state.drafts;
  const params = new URLSearchParams({
    limit: String(f.limit), offset: String(f.offset), status: f.status,
  });
  const data = await api("/api/admin/drafts?" + params.toString());

  const cards = data.items.map((d) => {
    const actions = d.status === "pending_approval"
      ? '<div class="action-row">' +
          '<button class="btn primary sm" data-approve="' + esc(d.id) + '">' +
            icon("check") + "Одобрить и отправить</button>" +
          '<button class="btn danger sm" data-reject="' + esc(d.id) + '">Отклонить</button>' +
        "</div>"
      : "";
    return [
      '<div class="card section">',
      '<div class="toolbar" style="margin-bottom:8px">',
        '<b>' + esc(d.subject || "(без темы)") + "</b>",
        draftBadge(d.status),
        '<span class="spacer"></span>',
        '<span class="muted small">' + esc(fmtDate(d.created_at)) + "</span>",
      "</div>",
      '<div class="kv small">Кому: ' + esc((d.to_addresses || []).join(", ")) + "</div>",
      '<details style="margin-top:8px"><summary class="link small" style="cursor:pointer">Показать текст ответа</summary>' +
        '<div class="detail-body" style="margin-top:8px">' + esc(d.body) + "</div></details>",
      actions,
      "</div>",
    ].join("");
  }).join("");

  $("#main").innerHTML = [
    head("Черновики", "Ответы, которые ИИ подготовил, но ещё не отправил. " +
      "Одобренное уйдёт получателю, отклонённое — исчезнет."),
    '<div class="toolbar">',
      select("d-status", "Показать", f.status, [
        ["pending_approval", "ждут решения"],
        ["approved", "одобренные"],
        ["rejected", "отклонённые"],
        ["sent", "отправленные"],
        ["", "все"],
      ]),
      '<span class="spacer"></span>',
      '<span class="muted small">найдено: ' + fmtNum(data.total) + "</span>",
    "</div>",
    cards || emptyState("Черновиков такого вида нет", "draft"),
    pager("drafts", f.offset, f.limit, data.total),
  ].join("");

  $("#d-status").addEventListener("change", () => {
    state.drafts.status = $("#d-status").value;
    state.drafts.offset = 0;
    renderDrafts();
  });
  bindPager("drafts", renderDrafts);

  bind("[data-approve]", async (btn) => {
    const r = await api("/api/admin/drafts/" + btn.dataset.approve + "/approve", { method: "POST" });
    toast(r.message || "Одобрен");
    await loadBadges();
    renderDrafts();
  });
  bind("[data-reject]", async (btn) => {
    confirmAction("Отклонить черновик?", "Ответ не будет отправлен получателю.", async () => {
      const r = await api("/api/admin/drafts/" + btn.dataset.reject + "/reject", { method: "POST" });
      toast(r.message || "Отклонён");
      await loadBadges();
      renderDrafts();
    });
    // confirmAction асинхронен — кнопка разблокируется сразу, рендер вызовет onOk.
  });
}

/* ---------- Лиды ---------- */

async function renderLeads() {
  const f = state.leads;
  const data = await api("/api/admin/leads?limit=" + f.limit + "&offset=" + f.offset);
  const rows = data.items.map((l) =>
    "<tr>" +
    td("Категория", esc(CATEGORY_LABELS[l.category] || l.category || "—")) +
    td("Компания", esc(l.company_name || l.contact_email || "—")) +
    td("Статус", '<span class="badge info">' + esc(l.status) + "</span>") +
    td("Область", esc(l.scope || "—")) +
    td("Уверенность", esc(l.confidence !== null && l.confidence !== undefined
      ? Math.round(l.confidence * 100) + "%" : "—"), "num") +
    td("Звонок", l.callback_requested ? '<span class="badge warn">звонок</span>' : "—") +
    td("Активность", esc(fmtDate(l.last_activity_at)), "num") +
    "</tr>").join("");

  $("#main").innerHTML = [
    head("Лиды", "Заявки и переписка, которые ИИ разобрал как потенциальных клиентов"),
    table(
      [{ t: "Категория" }, { t: "Компания" }, { t: "Статус" }, { t: "Область" },
       { t: "Уверенность", num: true }, { t: "Звонок" }, { t: "Активность", num: true }],
      rows, "лидов пока нет", "users"),
    pager("leads", f.offset, f.limit, data.total),
  ].join("");
  bindPager("leads", renderLeads);
}

/* ---------- Модели ---------- */

async function renderModels() {
  const [profiles, presets] = await Promise.all([
    api("/api/admin/llm/profiles"),
    api("/api/admin/llm/presets"),
  ]);

  const cards = profiles.map((p) => [
    '<div class="card section"' + (p.is_active ? ' style="border-color:var(--accent)"' : "") + ">",
    '<div class="toolbar" style="margin-bottom:6px">',
      "<b>" + esc(p.name) + "</b>",
      p.is_active ? '<span class="badge info">активна</span>' : "",
      '<span class="spacer"></span>',
      '<span class="mono small muted">' + esc(p.key_prefix ? p.key_prefix + "…" : "?") + "</span>",
    "</div>",
    '<div class="kv small">',
      "Модель: <b>" + esc(p.model) + "</b><br>",
      "Адрес: " + esc(p.base_url) + "<br>",
      "Цены за 1M токенов: ввод $" + esc(p.price_prompt_per_million_usd.toFixed(2)) +
        " / вывод $" + esc(p.price_completion_per_million_usd.toFixed(2)),
    "</div>",
    '<div class="action-row">',
      p.is_active
        ? '<button class="btn sm" data-off="' + esc(p.id) + '">Выключить (вернуть конфиг)</button>'
        : '<button class="btn primary sm" data-on="' + esc(p.id) + '">' + icon("check") + "Включить</button>",
      '<button class="btn sm" data-edit="' + esc(p.id) + '">Изменить</button>',
      '<button class="btn danger sm" data-del="' + esc(p.id) + '">Удалить</button>',
    "</div>",
    "</div>",
  ].join("")).join("");

  $("#main").innerHTML = [
    head("Модели", "Какая ИИ-модель отвечает за письма. Включённый профиль " +
      "перекрывает настройки из конфигурации за несколько секунд, без перезапуска."),
    cards || emptyState("Профилей нет — работает модель из конфигурации", "cpu"),

    "<h3>Добавить профиль</h3>",
    '<div class="card">',
      '<div class="toolbar">',
        select("np-preset", "Готовый пресет", "", [["", "— выберите —"]].concat(
          presets.map((p, i) => [String(i), p.label]))),
        '<button class="btn sm" id="np-fill">Заполнить</button>',
      "</div>",
      '<div class="form-grid">',
        field("np-name", "Название профиля", ""),
        field("np-model", "Модель", ""),
        field("np-url", "Адрес API (Base URL)", ""),
        field("np-key", "API-ключ", "", "password"),
        field("np-pprice", "Цена ввода, $ за 1M", "0"),
        field("np-cprice", "Цена вывода, $ за 1M", "0"),
      "</div>",
      '<div class="action-row"><button class="btn primary" id="np-save">Создать профиль</button></div>',
      '<p id="np-msg" class="small" style="margin:10px 0 0"></p>',
    "</div>",
  ].join("");

  $("#np-fill").addEventListener("click", () => {
    const raw = $("#np-preset").value;
    if (raw === "") return;
    const p = presets[Number(raw)];
    if (!p) return;
    $("#np-name").value = p.label;
    $("#np-model").value = p.model;
    $("#np-url").value = p.base_url;
    $("#np-pprice").value = String(p.price_prompt_per_million_usd);
    $("#np-cprice").value = String(p.price_completion_per_million_usd);
  });

  $("#np-save").addEventListener("click", async () => {
    const msg = $("#np-msg");
    msg.textContent = "";
    try {
      await api("/api/admin/llm/profiles", {
        method: "POST",
        body: {
          name: $("#np-name").value.trim(),
          model: $("#np-model").value.trim(),
          base_url: $("#np-url").value.trim(),
          api_key: $("#np-key").value.trim(),
          price_prompt_per_million_usd: Number($("#np-pprice").value || 0),
          price_completion_per_million_usd: Number($("#np-cprice").value || 0),
        },
      });
      toast("Профиль создан");
      renderModels();
    } catch (err) {
      msg.textContent = err.message;
      msg.className = "error small";
    }
  });

  bind("[data-on]", async (btn) => {
    await api("/api/admin/llm/profiles/" + btn.dataset.on + "/activate", { method: "POST" });
    toast("Профиль включён — новые вызовы пойдут на него");
    renderModels();
  });
  bind("[data-off]", async () => {
    await api("/api/admin/llm/profiles/deactivate", { method: "POST" });
    toast("Профиль выключен — работает модель из конфигурации");
    renderModels();
  });
  bind("[data-del]", async (btn) => {
    confirmAction("Удалить профиль?", "Если он активен, вернётся модель из конфигурации.", async () => {
      await api("/api/admin/llm/profiles/" + btn.dataset.del, { method: "DELETE" });
      toast("Профиль удалён");
      renderModels();
    });
  });
  bind("[data-edit]", async (btn) => {
    const p = profiles.find((x) => x.id === btn.dataset.edit);
    if (!p) return;
    openModal({
      title: "Изменить профиль",
      body: '<div class="form-grid">' +
        field("e-name", "Название", p.name) +
        field("e-model", "Модель", p.model) +
        field("e-url", "Адрес API", p.base_url) +
        field("e-key", "Новый API-ключ (пусто — оставить)", "", "password") +
        field("e-pprice", "Цена ввода, $ за 1M", String(p.price_prompt_per_million_usd)) +
        field("e-cprice", "Цена вывода, $ за 1M", String(p.price_completion_per_million_usd)) +
        "</div>",
      okText: "Сохранить",
      onOk: async () => {
        await api("/api/admin/llm/profiles/" + p.id, {
          method: "PATCH",
          body: {
            name: $("#m-e-name").value.trim(),
            model: $("#m-e-model").value.trim(),
            base_url: $("#m-e-url").value.trim(),
            api_key: $("#m-e-key").value.trim(),
            price_prompt_per_million_usd: Number($("#m-e-pprice").value || 0),
            price_completion_per_million_usd: Number($("#m-e-cprice").value || 0),
          },
        });
        toast("Профиль обновлён");
        renderModels();
      },
    });
  });
}

function field(id, label, value, type) {
  return "<label>" + esc(label) +
    '<input id="' + id + '" type="' + (type || "text") + '" value="' + esc(value || "") + '"></label>';
}

/* ---------- Затраты ---------- */

async function renderUsage() {
  const u = await api("/api/admin/usage?days=" + state.usageDays);
  const dayRows = u.by_day.map((d) =>
    "<tr>" +
    td("День", esc(d.day)) +
    td("Вызовы", esc(fmtNum(d.calls)), "num") +
    td("Ввод", esc(fmtNum(d.prompt_tokens)), "num") +
    td("Вывод", esc(fmtNum(d.completion_tokens)), "num") +
    td("Затраты", esc(fmtMoney(d.cost_micros_usd)), "num") +
    "</tr>").join("");
  const modelRows = u.by_model.map((m) =>
    "<tr>" +
    td("Модель", esc(m.model)) +
    td("Вызовы", esc(fmtNum(m.calls)), "num") +
    td("Токены", esc(fmtNum(m.prompt_tokens + m.completion_tokens)), "num") +
    td("Затраты", esc(fmtMoney(m.cost_micros_usd)), "num") +
    "</tr>").join("");
  const agentRows = u.by_agent.map((a) =>
    "<tr>" +
    td("Агент", esc(a.agent)) +
    td("Вызовов", esc(fmtNum(a.total)), "num") +
    td("Успешно", esc(fmtNum(a.succeeded)), "num") +
    td("Ошибок", esc(fmtNum(a.failed)), "num") +
    td("Токены", esc(fmtNum(a.tokens)), "num") +
    td("Затраты", esc(fmtMoney(a.cost_micros_usd)), "num") +
    "</tr>").join("");

  $("#main").innerHTML = [
    head("Затраты", "Сколько стоит ИИ: вызовы, токены и деньги по дням, моделям и агентам"),
    '<div class="toolbar">',
      select("u-days", "Период", String(state.usageDays),
        [["7", "7 дней"], ["30", "30 дней"], ["90", "90 дней"], ["365", "год"]]),
    "</div>",
    '<div class="cards section">',
      metric(fmtMoney(u.total.cost_micros_usd), "за период", "accent"),
      metric(fmtNum(u.total.calls), "вызовов"),
      metric(fmtNum(u.total.prompt_tokens), "токенов ввода"),
      metric(fmtNum(u.total.completion_tokens), "токенов вывода"),
    "</div>",
    "<h3>По дням</h3>",
    table([{ t: "День" }, { t: "Вызовы", num: true }, { t: "Ввод", num: true },
      { t: "Вывод", num: true }, { t: "Затраты", num: true }],
      dayRows, "за период вызовов не было", "money"),
    "<h3>По моделям</h3>",
    table([{ t: "Модель" }, { t: "Вызовы", num: true }, { t: "Токены", num: true },
      { t: "Затраты", num: true }],
      modelRows, "данных нет", "cpu"),
    "<h3>По агентам (за всё время)</h3>",
    table([{ t: "Агент" }, { t: "Вызовов", num: true }, { t: "Успешно", num: true },
      { t: "Ошибок", num: true }, { t: "Токены", num: true }, { t: "Затраты", num: true }],
      agentRows, "данных нет", "activity"),
  ].join("");

  $("#u-days").addEventListener("change", () => {
    state.usageDays = Number($("#u-days").value);
    renderUsage();
  });
}

/* ---------- Журнал ---------- */

async function renderJournal() {
  const f = state.events;
  const [events, outbox] = await Promise.all([
    api("/api/admin/events?limit=" + f.limit + "&offset=" + f.offset),
    api("/api/admin/outbox?limit=30"),
  ]);

  const eventRows = events.items.map((e) =>
    "<tr>" +
    td("Время", esc(fmtDate(e.created_at)), "num") +
    td("Уровень", severityBadge(e.severity)) +
    td("Код", '<span class="mono small">' + esc(e.code) + "</span>") +
    td("Сообщение", esc(e.message)) +
    td("Стадия", '<span class="small muted">' + esc(e.stage || "") + "</span>") +
    "</tr>").join("");

  const outRows = outbox.map((o) => {
    const cls = o.status === "sent" ? "ok"
      : o.status === "held" || o.status === "failed" ? "danger" : "info";
    const retry = o.status === "failed" || o.status === "held"
      ? '<button class="btn sm" data-retry="' + esc(o.id) + '">' + icon("refresh") + "Повторить</button>"
      : "";
    return "<tr>" +
      td("Тип", esc(o.message_type)) +
      td("Кому", esc(o.recipient)) +
      td("Тема", esc(o.subject || "—")) +
      td("Статус", '<span class="badge ' + cls + '">' + esc(o.status) + "</span>") +
      td("Попытки", esc(o.attempts + "/" + o.max_attempts), "num") +
      td("Отправлено", esc(fmtDate(o.sent_at || o.created_at)), "num") +
      td("", retry, "actions") +
      "</tr>";
  }).join("");

  $("#main").innerHTML = [
    head("Журнал", "События обработки и очередь отправки: что произошло и что уйдёт наружу"),

    '<div class="card section">',
      "<h3 style=\"margin-top:0\">Сервисные действия</h3>",
      '<div class="quick-actions">',
        '<button class="btn" id="j-recover">' + icon("inbox") + "Снять зависшие обработки</button>",
        '<button class="btn danger" id="j-purge">Очистить старые события</button>',
      "</div>",
      '<p class="muted small" style="margin:12px 0 0">«Снять зависшие» возвращает в очередь письма, ' +
        "оставшиеся в «обрабатывается» после сбоя. Очистка удаляет события старше указанного срока.</p>",
    "</div>",

    "<h3>События</h3>",
    table([{ t: "Время", num: true }, { t: "Уровень" }, { t: "Код" },
      { t: "Сообщение" }, { t: "Стадия" }],
      eventRows, "событий нет", "activity"),
    pager("events", f.offset, f.limit, events.total),

    "<h3>Исходящая очередь</h3>",
    table([{ t: "Тип" }, { t: "Кому" }, { t: "Тема" }, { t: "Статус" },
      { t: "Попытки", num: true }, { t: "Отправлено", num: true }, { t: "" }],
      outRows, "очередь пуста — ничего не ждёт отправки", "send"),
  ].join("");

  bindPager("events", renderJournal);

  bind("#j-recover", async () => {
    const r = await api("/api/admin/ops/recover", { method: "POST" });
    toast(r.recovered > 0 ? "Снято с зависания: " + r.recovered : "Зависших писем нет");
  });
  bind("#j-purge", () => {
    openModal({
      title: "Очистить старые события",
      body: "<p style=\"margin:0 0 12px;color:var(--muted)\">Будут удалены события старше указанного " +
        "количества дней. Свежие останутся.</p>" +
        modalField("purge-days", "Хранить события за, дней", "30", "number"),
      okText: "Удалить",
      onOk: async () => {
        const days = Number($("#m-purge-days").value || 30);
        const r = await api("/api/admin/ops/purge-events?older_than_days=" + days, { method: "POST" });
        toast("Удалено событий: " + r.purged);
        renderJournal();
      },
    });
  });
  bind("[data-retry]", async (btn) => {
    const r = await api("/api/admin/outbox/" + btn.dataset.retry + "/retry", { method: "POST" });
    toast(r.queued ? "Поставлено в очередь на повторную отправку" : "Не удалось");
    renderJournal();
  });
}

/* ---------- запуск ---------- */

bindLogin();
bindModal();
checkSession();

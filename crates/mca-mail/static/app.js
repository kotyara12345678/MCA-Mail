/* Панель MCA Mail: hash-роутинг на чистом JS, без сборки.
   Правило файла: всё, что пришло с сервера, проходит через esc(). */

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

/* ---------- состояние ---------- */

const state = {
  page: "dashboard",
  emailId: null,
  emails: { offset: 0, limit: 30, status: "", category: "", flagged: "", search: "" },
  leads: { offset: 0, limit: 30 },
  events: { offset: 0, limit: 40 },
  usageDays: 30,
  session: null,
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

const SEVERITY_LABELS = {
  debug: "debug",
  info: "info",
  warning: "warning",
  error: "error",
};

function statusBadge(status) {
  const cls = status === "processed" ? "ok"
    : status === "failed" || status === "dead" ? "danger"
    : status === "needs_review" || status === "pending" ? "warn"
    : "info";
  return '<span class="badge ' + cls + '">' + esc(STATUS_LABELS[status] || status) + "</span>";
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
  route();
}

function bindLogin() {
  $("#login-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const errorBox = $("#login-error");
    errorBox.classList.add("hidden");
    try {
      state.session = await api("/api/auth/login", {
        method: "POST",
        body: {
          login: $("#login-input").value.trim(),
          password: $("#login-password").value,
        },
      });
      $("#login-password").value = "";
      showApp();
    } catch (err) {
      errorBox.textContent = err.message;
      errorBox.classList.remove("hidden");
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
  leads: renderLeads,
  models: renderModels,
  usage: renderUsage,
  pipeline: renderPipeline,
  events: renderEvents,
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
  $("#main").innerHTML = '<div class="muted">Загрузка…</div>';
  render().catch((err) => {
    $("#main").innerHTML = '<p class="error">' + esc(err.message) + "</p>";
  });
}

window.addEventListener("hashchange", route);

/* ---------- дашборд ---------- */

async function renderDashboard() {
  const s = await api("/api/admin/summary");
  const counts = s.emails.by_status || {};
  const pipe = s.pipeline;
  const chips = [
    chip(!pipe.poll_paused, "получение почты"),
    chip(!pipe.queue_paused, "обработка"),
    chip(!pipe.outbox_paused, "отправка"),
    chip(pipe.automation_enabled, "автоматика"),
  ].join("");

  $("#main").innerHTML = [
    '<h2 class="page">Дашборд</h2>',
    '<div class="cards section">',
      metric(fmtNum(s.emails.total), "писем всего"),
      metric(fmtNum(s.emails.flagged), "флаг «заказ»", "flagged"),
      metric(fmtNum(counts.pending || 0), "в очереди"),
      metric(fmtNum(s.leads_total), "лидов"),
      metric(fmtNum(s.drafts_pending), "черновиков на проверку"),
      metric(fmtNum(s.outbox_pending), "исходящих в очереди"),
      metric(fmtMoney(s.cost_today_micros), "затрат сегодня"),
      metric(fmtMoney(s.cost_total_micros), "затрат всего"),
    "</div>",
    '<div class="card section">',
      '<h3 style="margin-top:0">Пайплайн</h3>',
      '<div class="chips">' + chips + "</div>",
      '<p class="muted small" style="margin-bottom:0">Аптайм: ' + esc(fmtDuration(s.uptime_seconds)) +
        " · база данных: " + (s.db_ok ? "на связи" : "недоступна") + "</p>",
    "</div>",
    '<div class="card section">',
      '<h3 style="margin-top:0">Модель</h3>',
      "<p style='margin:0'>Активная: <b>" + esc(s.llm.active_model || "—") + "</b>" +
        (s.llm.profile
          ? ' <span class="badge info">профиль: ' + esc(s.llm.profile.name) + "</span>"
          : ' <span class="muted small">из конфигурации: ' + esc(s.llm.config_model) + "</span>") +
      "</p>",
      '<p class="muted small" style="margin:6px 0 0">Провайдер: ' + esc(s.llm.provider) + "</p>",
    "</div>",
    '<div class="card">',
      '<h3 style="margin-top:0">Быстрый переход</h3>',
      '<p style="margin:0"><a class="link" href="#/emails">Письма</a> · ' +
      '<a class="link" href="#/pipeline">Паузы пайплайна</a> · ' +
      '<a class="link" href="#/models">Смена модели</a> · ' +
      '<a class="link" href="#/usage">Стоимость</a></p>',
    "</div>",
  ].join("");
}

function metric(value, label, extraClass) {
  return '<div class="card metric ' + (extraClass || "") + '">' +
    '<div class="value">' + value + "</div>" +
    '<div class="label">' + esc(label) + "</div></div>";
}

function chip(on, label) {
  return '<span class="chip ' + (on ? "on" : "paused") + '">' +
    (on ? "▶ " : "⏸ ") + esc(label) + "</span>";
}

/* ---------- письма ---------- */

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
      "<td>" + flag + '<a class="link" href="#/emails/' + esc(row.id) + '">' + esc(row.subject || "(без темы)") + "</a></td>" +
      "<td>" + esc(row.from_address) + "</td>" +
      "<td>" + statusBadge(row.status) + "</td>" +
      "<td>" + esc(CATEGORY_LABELS[row.category] || row.category || "—") + "</td>" +
      "<td class='num'>" + esc(fmtDate(row.received_at)) + "</td>" +
      "</tr>";
  }).join("");

  $("#main").innerHTML = [
    '<h2 class="page">Письма</h2>',
    '<div class="toolbar">',
      select("f-status", "Статус:", f.status, Object.entries({ "": "все" }).concat(
        Object.entries(STATUS_LABELS))),
      select("f-category", "Категория:", f.category, Object.entries({ "": "все" }).concat(
        Object.entries(CATEGORY_LABELS).filter(([k]) => k !== "no"))),
      select("f-flagged", "Флаг «заказ»:", f.flagged,
        [["", "все"], ["true", "с флагом"], ["false", "без флага"]]),
      '<input type="search" id="f-search" placeholder="поиск по теме или отправителю" value="' + esc(f.search) + '">',
      '<button class="btn" id="f-apply">Найти</button>',
      '<span class="spacer"></span>',
      '<span class="muted small">найдено: ' + fmtNum(data.total) + "</span>",
    "</div>",
    '<div class="table-wrap"><table>',
      "<thead><tr><th>Тема</th><th>Отправитель</th><th>Статус</th><th>Категория</th><th class='num'>Получено</th></tr></thead>",
      "<tbody>" + (rows || '<tr><td colspan="5" class="muted">Ничего не найдено</td></tr>') + "</tbody>",
    "</table></div>",
    pager("emails", f.offset, f.limit, data.total),
  ].join("");

  $("#f-apply").addEventListener("click", () => {
    f.status = $("#f-status").value;
    f.category = $("#f-category").value;
    f.flagged = $("#f-flagged").value;
    f.search = $("#f-search").value.trim();
    f.offset = 0;
    renderEmails();
  });
  $("#f-search").addEventListener("keydown", (e) => {
    if (e.key === "Enter") $("#f-apply").click();
  });
  bindPager("emails");
}

function select(id, label, current, options) {
  const opts = options.map(([value, text]) =>
    '<option value="' + esc(value) + '"' + (value === current ? " selected" : "") + ">" +
    esc(text) + "</option>").join("");
  return '<label class="muted small">' + esc(label) +
    ' <select id="' + id + '">' + opts + "</select></label>";
}

function pager(scope, offset, limit, total) {
  const prevDisabled = offset <= 0 ? " disabled" : "";
  const nextDisabled = offset + limit >= total ? " disabled" : "";
  const from = total === 0 ? 0 : offset + 1;
  const to = Math.min(offset + limit, total);
  return '<div class="pager">' +
    '<button class="btn" data-pager="' + scope + '" data-dir="-1"' + prevDisabled + ">← назад</button>" +
    '<button class="btn" data-pager="' + scope + '" data-dir="1"' + nextDisabled + ">вперёд →</button>" +
    '<span class="muted small">' + from + "–" + to + " из " + fmtNum(total) + "</span>" +
    "</div>";
}

function bindPager(scope) {
  document.querySelectorAll('[data-pager="' + scope + '"]').forEach((btn) => {
    btn.addEventListener("click", () => {
      const dir = Number(btn.dataset.dir);
      state[scope].offset = Math.max(0, state[scope].offset + dir * state[scope].limit);
      if (scope === "emails") renderEmails();
      if (scope === "leads") renderLeads();
      if (scope === "events") renderEvents();
    });
  });
}

async function renderEmailDetail(id) {
  const [email, runs] = await Promise.all([
    api("/api/v1/emails/" + encodeURIComponent(id)),
    api("/api/admin/emails/" + encodeURIComponent(id) + "/runs?limit=50").catch(() => ({ runs: [], agent_runs: [] })),
  ]);

  const runRows = (runs.runs || []).map((r) =>
    "<tr><td>" + esc(r.state) + "</td><td>" + esc(r.trigger) + "</td><td class='num'>" +
    esc(fmtNum(r.attempt)) + "</td><td class='num'>" + esc(fmtNum(r.total_tokens)) +
    "</td><td class='num'>" + esc(fmtMoney(r.cost_micros_usd)) + "</td><td class='num'>" +
    esc(fmtDate(r.started_at)) + "</td></tr>").join("");

  const agentRows = (runs.agent_runs || []).map((r) =>
    "<tr><td>" + esc(r.agent) + "</td><td>" + esc(r.model) + "</td><td>" + esc(r.state) +
    "</td><td class='num'>" + esc(fmtNum(r.prompt_tokens + r.completion_tokens)) +
    "</td><td class='num'>" + esc(fmtMoney(r.cost_micros_usd)) +
    "</td><td class='num'>" + esc(fmtDate(r.started_at)) + "</td></tr>").join("");

  $("#main").innerHTML = [
    '<p><a class="link" href="#/emails">← к списку писем</a></p>',
    '<h2 class="page">' + (email.order_flag ? '<span class="flag" title="заказ"></span>' : "") +
      esc(email.subject || "(без темы)") + "</h2>",
    '<div class="card section">',
      '<p class="muted" style="margin-top:0">' +
        "От: <b>" + esc(email.from_name || "") + " &lt;" + esc(email.from_address) + "&gt;</b><br>" +
        "Кому: " + esc((email.to_addresses || []).join(", ")) + "<br>" +
        "Получено: " + esc(fmtDate(email.received_at)) + (email.date ? " (дата письма: " + esc(fmtDate(email.date)) + ")" : "") +
      "</p>",
      "<p>" + statusBadge(email.status) +
        " " + '<span class="badge">' + esc(CATEGORY_LABELS[email.category] || email.category || "—") + "</span>" +
        (email.spam_verdict ? ' <span class="badge">спам: ' + esc(email.spam_verdict) + "</span>" : "") +
        (email.order_flag ? ' <span class="badge danger">заказ</span>' : "") +
        (email.lead_id ? ' <a class="link" href="#/leads">лид есть</a>' : "") +
      "</p>",
      '<div class="toolbar">',
        '<button class="btn" data-act="reprocess">Повторить обработку</button>',
        '<button class="btn" data-act="review">На проверку</button>',
        '<button class="btn primary" data-act="approve">Одобрить</button>',
        '<button class="btn danger" data-act="reject">Отклонить</button>',
      "</div>",
      '<div class="detail-body">' + esc(email.text_body || "(пусто)") + "</div>",
    "</div>",
    '<div class="section">',
      "<h3>Запуски пайплайна</h3>",
      table(["Состояние", "Триггер", "Попытка", "Токены", "Затраты", "Начато"], runRows,
        "запусков нет"),
    "</div>",
    '<div class="section">',
      "<h3>Вызовы моделей</h3>",
      table(["Агент", "Модель", "Состояние", "Токены", "Затраты", "Начато"], agentRows,
        "вызовов не было — затраты пишутся только для новых запусков"),
    "</div>",
  ].join("");

  document.querySelectorAll("[data-act]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      try {
        const res = await api(
          "/api/v1/emails/" + encodeURIComponent(id) + "/" + btn.dataset.act,
          { method: "POST" });
        toast(res.message || "Готово");
        renderEmailDetail(id);
      } catch (err) {
        toast(err.message, true);
        btn.disabled = false;
      }
    });
  });
}

function table(headers, rowsHtml, emptyText) {
  const head = headers.map((h, i) =>
    "<th" + (i >= 3 && i <= 4 ? " class='num'" : "") + ">" + esc(h) + "</th>").join("");
  const body = rowsHtml || '<tr><td colspan="' + headers.length + '" class="muted">' + esc(emptyText) + "</td></tr>";
  return '<div class="table-wrap"><table><thead><tr>' + head + "</tr></thead><tbody>" +
    body + "</tbody></table></div>";
}

/* ---------- лиды ---------- */

async function renderLeads() {
  const f = state.leads;
  const data = await api("/api/admin/leads?limit=" + f.limit + "&offset=" + f.offset);
  const rows = data.items.map((l) =>
    "<tr>" +
    "<td>" + esc(CATEGORY_LABELS[l.category] || l.category || "—") + "</td>" +
    "<td>" + esc(l.company_name || l.contact_email || "—") + "</td>" +
    "<td>" + esc(l.status) + "</td>" +
    "<td>" + esc(l.scope || "—") + "</td>" +
    "<td class='num'>" + esc(l.confidence !== null && l.confidence !== undefined ? Math.round(l.confidence * 100) + "%" : "—") + "</td>" +
    "<td>" + (l.callback_requested ? '<span class="badge warn">звонок</span>' : "") + "</td>" +
    "<td class='num'>" + esc(fmtDate(l.last_activity_at)) + "</td>" +
    "</tr>").join("");

  $("#main").innerHTML = [
    '<h2 class="page">Лиды</h2>',
    table(["Категория", "Компания", "Статус", "Область", "Уверенность", "Звонок", "Активность"],
      rows, "лидов пока нет"),
    pager("leads", f.offset, f.limit, data.total),
  ].join("");
  bindPager("leads");
}

/* ---------- модели ---------- */

async function renderModels() {
  const [profiles, presets] = await Promise.all([
    api("/api/admin/llm/profiles"),
    api("/api/admin/llm/presets"),
  ]);

  const rows = profiles.map((p) => {
    const key = p.key_prefix ? p.key_prefix + "…" : "?";
    return "<tr" + (p.is_active ? ' style="background:var(--accent-soft)"' : "") + ">" +
      "<td>" + esc(p.name) + (p.is_active ? ' <span class="badge info">активен</span>' : "") + "</td>" +
      "<td>" + esc(p.model) + "</td>" +
      "<td class='mono small'>" + esc(p.base_url) + "</td>" +
      "<td class='mono small'>" + esc(key) + "</td>" +
      "<td class='num'>" + esc(p.price_prompt_per_million_usd.toFixed(2)) + " / " +
        esc(p.price_completion_per_million_usd.toFixed(2)) + "</td>" +
      "<td class='num'>" +
        (p.is_active
          ? '<button class="btn" data-off="' + esc(p.id) + '">Выключить</button>'
          : '<button class="btn primary" data-on="' + esc(p.id) + '">Включить</button>') +
        ' <button class="btn" data-edit="' + esc(p.id) + '">Изменить</button>' +
        ' <button class="btn danger" data-del="' + esc(p.id) + '">Удалить</button>' +
      "</td></tr>";
  }).join("");

  const presetOptions = presets.map((p, i) =>
    '<option value="' + i + '">' + esc(p.label) + "</option>").join("");

  $("#main").innerHTML = [
    '<h2 class="page">Модели</h2>',
    '<p class="muted">Активный профиль заменяет модель, адрес и ключ для всех вызовов — ' +
      "переключение применяется в течение нескольких секунд, без перезапуска.</p>",
    table(["Профиль", "Модель", "Адрес", "Ключ", "$/M ввод / вывод", ""], rows,
      "профилей нет — работает модель из конфигурации"),

    "<h3>Новый профиль</h3>",
    '<div class="card">',
      '<div class="toolbar">',
        '<label class="muted small">Пресет: <select id="np-preset">' + presetOptions + "</select></label>",
        '<button class="btn" id="np-fill">Заполнить из пресета</button>',
      "</div>",
      '<div class="form-grid">',
        field("np-name", "Название", "DeepSeek Chat"),
        field("np-model", "Модель", "deepseek-chat"),
        field("np-url", "Base URL", "https://api.deepseek.com"),
        field("np-key", "API-ключ", "", "password"),
        field("np-pprice", "Цена ввода, $/1M", "0.27"),
        field("np-cprice", "Цена вывода, $/1M", "1.10"),
      "</div>",
      '<div class="toolbar" style="margin-top:12px"><button class="btn primary" id="np-save">Создать</button></div>',
      '<p id="np-msg" class="muted small"></p>',
    "</div>",
  ].join("");

  $("#np-fill").addEventListener("click", () => {
    const p = presets[Number($("#np-preset").value)];
    if (!p) return;
    $("#np-name").value = p.label;
    $("#np-model").value = p.model;
    $("#np-url").value = p.base_url;
    $("#np-pprice").value = String(p.price_prompt_per_million_usd);
    $("#np-cprice").value = String(p.price_completion_per_million_usd);
  });

  $("#np-save").addEventListener("click", async () => {
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
      $("#np-msg").textContent = err.message;
      $("#np-msg").className = "error small";
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
    if (!confirm("Удалить профиль?")) return;
    await api("/api/admin/llm/profiles/" + btn.dataset.del, { method: "DELETE" });
    toast("Профиль удалён");
    renderModels();
  });
  bind("[data-edit]", async (btn) => {
    const p = profiles.find((x) => x.id === btn.dataset.edit);
    if (!p) return;
    const name = prompt("Название профиля:", p.name);
    if (name === null) return;
    const model = prompt("Модель:", p.model);
    if (model === null) return;
    const url = prompt("Base URL:", p.base_url);
    if (url === null) return;
    const pp = prompt("Цена ввода, $/1M:", String(p.price_prompt_per_million_usd));
    if (pp === null) return;
    const cp = prompt("Цена вывода, $/1M:", String(p.price_completion_per_million_usd));
    if (cp === null) return;
    const key = prompt("Новый API-ключ (пусто — оставить текущий):", "");
    if (key === null) return;
    await api("/api/admin/llm/profiles/" + p.id, {
      method: "PATCH",
      body: {
        name: name.trim(), model: model.trim(), base_url: url.trim(),
        price_prompt_per_million_usd: Number(pp || 0),
        price_completion_per_million_usd: Number(cp || 0),
        api_key: key.trim(),
      },
    });
    toast("Профиль обновлён");
    renderModels();
  });
}

function field(id, label, value, type) {
  return "<label>" + esc(label) +
    '<input id="' + id + '" type="' + (type || "text") + '" value="' + esc(value || "") + '"></label>';
}

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

/* ---------- стоимость ---------- */

async function renderUsage() {
  const u = await api("/api/admin/usage?days=" + state.usageDays);
  const dayRows = u.by_day.map((d) =>
    "<tr><td>" + esc(d.day) + "</td><td class='num'>" + esc(fmtNum(d.calls)) +
    "</td><td class='num'>" + esc(fmtNum(d.prompt_tokens)) +
    "</td><td class='num'>" + esc(fmtNum(d.completion_tokens)) +
    "</td><td class='num'>" + esc(fmtMoney(d.cost_micros_usd)) + "</td></tr>").join("");
  const modelRows = u.by_model.map((m) =>
    "<tr><td>" + esc(m.model) + "</td><td class='num'>" + esc(fmtNum(m.calls)) +
    "</td><td class='num'>" + esc(fmtNum(m.prompt_tokens + m.completion_tokens)) +
    "</td><td class='num'>" + esc(fmtMoney(m.cost_micros_usd)) + "</td></tr>").join("");
  const agentRows = u.by_agent.map((a) =>
    "<tr><td>" + esc(a.agent) + "</td><td class='num'>" + esc(fmtNum(a.total)) +
    "</td><td class='num'>" + esc(fmtNum(a.succeeded)) +
    "</td><td class='num'>" + esc(fmtNum(a.failed)) +
    "</td><td class='num'>" + esc(fmtNum(a.tokens)) +
    "</td><td class='num'>" + esc(fmtMoney(a.cost_micros_usd)) + "</td></tr>").join("");

  $("#main").innerHTML = [
    '<h2 class="page">Стоимость</h2>',
    '<div class="toolbar">',
      select("u-days", "Период:", String(state.usageDays),
        [["7", "7 дней"], ["30", "30 дней"], ["90", "90 дней"], ["365", "год"]]),
      '<button class="btn" id="u-apply">Показать</button>',
    "</div>",
    '<div class="cards section">',
      metric(fmtMoney(u.total.cost_micros_usd), "за период"),
      metric(fmtNum(u.total.calls), "вызовов"),
      metric(fmtNum(u.total.prompt_tokens), "токенов ввода"),
      metric(fmtNum(u.total.completion_tokens), "токенов вывода"),
    "</div>",
    "<h3>По дням</h3>",
    table(["День", "Вызовы", "Ввод", "Вывод", "Затраты"], dayRows, "за период вызовов не было"),
    "<h3>По моделям</h3>",
    table(["Модель", "Вызовы", "Токены", "Затраты"], modelRows, "данных нет"),
    "<h3>По агентам (за всё время)</h3>",
    table(["Агент", "Вызовов", "Успешно", "Ошибок", "Токены", "Затраты"], agentRows, "данных нет"),
  ].join("");

  $("#u-apply").addEventListener("click", () => {
    state.usageDays = Number($("#u-days").value);
    renderUsage();
  });
}

/* ---------- пайплайн ---------- */

async function renderPipeline() {
  const [flags, outbox] = await Promise.all([
    api("/api/admin/settings"),
    api("/api/admin/outbox?limit=30"),
  ]);

  const outRows = outbox.map((o) =>
    "<tr><td>" + esc(o.message_type) + "</td><td>" + esc(o.recipient) + "</td>" +
    "<td>" + esc(o.subject || "—") + "</td><td>" + outStatus(o.status) + "</td>" +
    "<td class='num'>" + esc(o.attempts) + "/" + esc(o.max_attempts) + "</td>" +
    "<td class='num'>" + esc(fmtDate(o.sent_at || o.created_at)) + "</td></tr>").join("");

  $("#main").innerHTML = [
    '<h2 class="page">Пайплайн</h2>',
    '<div class="card section">',
      '<h3 style="margin-top:0">Переключатели</h3>',
      switchRow("sw-poll", "Пауза: получение почты", "IMAP-опрос стоит, письма не подтягиваются", flags.poll_paused),
      switchRow("sw-queue", "Пауза: обработка", "новые письма копятся в очереди, LLM не вызывается", flags.queue_paused),
      switchRow("sw-outbox", "Пауза: отправка", "исходящие копятся, ничего не уходит наружу", flags.outbox_paused),
      switchRow("sw-auto", "Автоматика включена", "автоответы и карточки менеджеру разрешены", flags.automation_enabled),
      '<div class="toolbar" style="margin-top:12px"><button class="btn primary" id="sw-save">Сохранить</button></div>',
      '<p class="muted small">Изменения применяются сразу, перезапуск не нужен.</p>',
    "</div>",
    '<div class="section">',
      "<h3>Исходящая очередь</h3>",
      table(["Тип", "Кому", "Тема", "Статус", "Попытки", "Отправлено"], outRows,
        "очередь пуста"),
    "</div>",
  ].join("");

  $("#sw-save").addEventListener("click", async () => {
    try {
      await api("/api/admin/settings", {
        method: "PATCH",
        body: {
          poll_paused: $("#sw-poll").checked,
          queue_paused: $("#sw-queue").checked,
          outbox_paused: $("#sw-outbox").checked,
          automation_enabled: $("#sw-auto").checked,
        },
      });
      toast("Сохранено");
      renderPipeline();
    } catch (err) {
      toast(err.message, true);
    }
  });
}

function switchRow(id, label, desc, checked) {
  return '<div class="switch-row"><div><div>' + esc(label) + "</div>" +
    '<div class="desc">' + esc(desc) + "</div></div>" +
    '<input type="checkbox" id="' + id + '"' + (checked ? " checked" : "") + "></div>";
}

function outStatus(status) {
  const cls = status === "sent" ? "ok" : status === "held" || status === "failed" ? "danger" : "info";
  return '<span class="badge ' + cls + '">' + esc(status) + "</span>";
}

/* ---------- события ---------- */

async function renderEvents() {
  const f = state.events;
  const data = await api("/api/admin/events?limit=" + f.limit + "&offset=" + f.offset);
  const rows = data.items.map((e) =>
    "<tr><td class='num'>" + esc(fmtDate(e.created_at)) + "</td>" +
    "<td>" + severityBadge(e.severity) + "</td>" +
    "<td class='mono small'>" + esc(e.code) + "</td>" +
    "<td>" + esc(e.message) + "</td>" +
    "<td class='small muted'>" + esc(e.stage || "") + "</td></tr>").join("");

  $("#main").innerHTML = [
    '<h2 class="page">События</h2>',
    table(["Время", "Уровень", "Код", "Сообщение", "Стадия"], rows, "событий нет"),
    pager("events", f.offset, f.limit, data.total),
  ].join("");
  bindPager("events");
}

/* ---------- запуск ---------- */

bindLogin();
checkSession();

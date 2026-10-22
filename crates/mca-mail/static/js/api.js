// Typed-ish API client for the MCA Mail panel.
//
// One place owns fetch, JSON encoding, cookie credentials and error shaping,
// so views never touch `fetch` and never invent their own error handling.
// Every function maps 1:1 to a backend route; the shapes it returns are the
// shapes the Rust handlers serialize.

export class ApiError extends Error {
  constructor(status, code, message) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
  }
}

async function request(method, path, body) {
  const init = {
    method,
    credentials: 'same-origin',
    headers: {},
  };
  if (body !== undefined) {
    init.headers['content-type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const res = await fetch(path, init);
  const text = await res.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = { message: text };
    }
  }
  if (!res.ok) {
    throw new ApiError(
      res.status,
      (data && data.code) || 'error',
      (data && data.message) || `HTTP ${res.status}`,
    );
  }
  return data;
}

function query(params) {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== '') {
      search.set(key, String(value));
    }
  }
  const encoded = search.toString();
  return encoded ? `?${encoded}` : '';
}

export const api = {
  // session
  session: () => request('GET', '/api/auth/session'),
  login: (login, password) => request('POST', '/api/auth/login', { login, password }),
  logout: () => request('POST', '/api/auth/logout'),

  // dialog feed
  threads: (params = {}) => request('GET', `/api/admin/threads${query(params)}`),
  thread: (id, params = {}) =>
    request('GET', `/api/admin/threads/${encodeURIComponent(id)}${query(params)}`),

  // drafts
  approveDraft: (id) => request('POST', `/api/admin/drafts/${encodeURIComponent(id)}/approve`),
  rejectDraft: (id) => request('POST', `/api/admin/drafts/${encodeURIComponent(id)}/reject`),

  // settings & agent mode
  settings: () => request('GET', '/api/admin/settings'),
  patchSettings: (patch) => request('PATCH', '/api/admin/settings', patch),

  // dashboard
  summary: () => request('GET', '/api/admin/summary'),

  // model profiles
  profiles: () => request('GET', '/api/admin/llm/profiles'),
  createProfile: (body) => request('POST', '/api/admin/llm/profiles', body),
  updateProfile: (id, body) =>
    request('PATCH', `/api/admin/llm/profiles/${encodeURIComponent(id)}`, body),
  deleteProfile: (id) => request('DELETE', `/api/admin/llm/profiles/${encodeURIComponent(id)}`),
  activateProfile: (id) =>
    request('POST', `/api/admin/llm/profiles/${encodeURIComponent(id)}/activate`),
  deactivateProfiles: () => request('POST', '/api/admin/llm/profiles/deactivate'),
  presets: () => request('GET', '/api/admin/llm/presets'),

  // costs
  usage: (days = 30) => request('GET', `/api/admin/usage${query({ days })}`),

  // service actions
  pollNow: () => request('POST', '/api/admin/ops/poll-now'),
  recover: () => request('POST', '/api/admin/ops/recover'),
  purgeEvents: (olderThanDays) =>
    request('POST', `/api/admin/ops/purge-events${query({ older_than_days: olderThanDays })}`),
};

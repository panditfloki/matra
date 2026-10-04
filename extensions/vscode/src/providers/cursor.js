'use strict';

const { loadCredentials } = require('./cursor-auth');
const { TTL_MS, finiteNumber, epoch, safePlan, fetchJson, createCache, retryAt } = require('./cursor-runtime');

const ENDPOINT = 'https://cursor.com/api/usage-summary';

function parseUsage(body) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) return null;
  const resetsAt = epoch(body.billingCycleEnd);
  const usage = body.individualUsage || {};
  const plan = usage.plan || {};
  const windows = [];
  function percentage(value, id, label) {
    const percent = finiteNumber(value);
    if (percent !== null && percent >= 0) windows.push({ id, label, usedPercent: percent, resetsAt });
  }
  function spend(bucket, id, label) {
    const used = finiteNumber(bucket?.used);
    const limit = finiteNumber(bucket?.limit);
    if (bucket?.enabled === true && used !== null && used >= 0 && limit !== null && limit > 0) {
      windows.push({ id, label, usedPercent: used / limit * 100, resetsAt });
    }
  }
  // Explicit percentages are authoritative even when used/limit are both zero.
  percentage(plan.autoPercentUsed, 'auto', 'Auto usage');
  percentage(plan.apiPercentUsed, 'api', 'API usage');
  if (!windows.length) spend(usage.overall, 'included', 'Included usage');
  spend(usage.onDemand, 'on_demand', 'On demand');
  spend(body.teamUsage?.onDemand, 'team_on_demand', 'Team on demand');
  const headlineId = windows.some(w => w.id === 'auto') ? 'auto'
    : windows.some(w => w.id === 'included') ? 'included'
      : windows.some(w => w.id === 'api') ? 'api' : windows[0]?.id || null;
  return {
    windows, headlineId, plan: safePlan(body.membershipType), unlimited: body.isUnlimited === true
  };
}

function empty(status, message, account = { label: null, plan: null }, source = 'Cursor editor') {
  return { id: 'cursor', name: 'Cursor', status, message, source, updatedAt: null,
    account, headlineId: null, windows: [] };
}

function authFailure(code) {
  if (code === 'sqlite-unavailable') return empty('unavailable',
    'Cursor credentials need Node SQLite support, sqlite3, or Python 3 with sqlite3. Install one and refresh.');
  if (code === 'database-failed') return empty('unavailable',
    'The Cursor sign-in database could not be read. Check access to Cursor local data and refresh.');
  if (code === 'keychain-unavailable') return empty('unavailable',
    'The cursor-agent keychain read timed out. Unlock the login keychain or sign in through Cursor, then refresh.');
  return empty('needs-auth', code === 'expired-auth'
    ? 'The Cursor session has expired. Sign in again in Cursor or run cursor-agent login, then refresh.'
    : 'Sign in to the Cursor editor, then refresh. On macOS, an existing cursor-agent login is also supported.');
}

function createProvider(options = {}) {
  const cache = createCache(options.storagePath, 'cursor');
  const fetchImpl = options.fetch || globalThis.fetch;
  let cooldown = 0;
  let currentKey = null;
  const pending = new Map();

  async function read({ now = Date.now(), force = false } = {}) {
    let auth;
    try { auth = await loadCredentials(options, now); }
    catch (error) { currentKey = null; await cache.clear(); return authFailure(error.code); }
    if (currentKey !== null && currentKey !== auth.key) { await cache.clear(); cooldown = 0; }
    currentKey = auth.key;
    const saved = await cache.get(auth.key);
    if (!force && saved && now >= saved.updatedAt && now - saved.updatedAt < TTL_MS) return saved;
    if (now < cooldown) return saved
      ? { ...saved, status: 'stale', message: 'Cursor rate limited this reader. The last observation is shown; retry later.' }
      : empty('unavailable', 'Cursor rate limited this reader. Wait a few minutes and refresh.', auth.account, auth.source);
    if (pending.has(auth.key)) return pending.get(auth.key);
    const operation = (async () => {
      let response;
      try {
        if (typeof fetchImpl !== 'function') throw new Error('no transport');
        response = await fetchJson(fetchImpl, ENDPOINT, {
          method: 'GET', headers: { Accept: 'application/json', Cookie: auth.cookie }
        }, 15000);
      } catch {
        let checked;
        try { checked = await loadCredentials(options, now); }
        catch (error) { currentKey = null; await cache.clear(); return authFailure(error.code); }
        if (checked.key !== auth.key || currentKey !== auth.key) {
          await cache.clear();
          return empty('unavailable', 'The Cursor account changed while refreshing. Refresh again for the current account.');
        }
        return saved
          ? { ...saved, status: 'stale', message: 'Cursor usage could not be reached. The last observation is shown; refresh after checking connectivity.' }
          : empty('unavailable', 'Cursor usage could not be reached. Check connectivity and refresh.', auth.account, auth.source);
      }

      // A sign-out or account switch during the request invalidates that response too.
      let checked;
      try { checked = await loadCredentials(options, now); }
      catch (error) { currentKey = null; await cache.clear(); return authFailure(error.code); }
      if (checked.key !== auth.key || currentKey !== auth.key) {
        await cache.clear();
        return empty('unavailable', 'The Cursor account changed while refreshing. Refresh again for the current account.');
      }
      if (response.status === 401 || response.status === 403) {
        await cache.clear();
        return empty('needs-auth', 'Cursor rejected the saved session. Sign in again in Cursor, then refresh.', checked.account, checked.source);
      }
      if (response.status === 429) cooldown = retryAt(response.retryAfter, now);
      if (response.status < 200 || response.status >= 300) return saved
        ? { ...saved, status: 'stale', message: response.status === 429
          ? 'Cursor rate limited this reader. The last observation is shown; retry later.'
          : 'Cursor returned a temporary service error. The last observation is shown; refresh later.' }
        : empty('unavailable', response.status === 429
          ? 'Cursor rate limited this reader. Wait a few minutes and refresh.'
          : 'Cursor usage is unavailable from the service. Refresh later.', checked.account, checked.source);
      const parsed = parseUsage(response.body);
      if (!parsed) return saved
        ? { ...saved, status: 'stale', message: 'Cursor returned an unreadable usage response. The last observation is shown.' }
        : empty('error', 'Cursor returned an unreadable usage response. Refresh later.', checked.account, checked.source);
      const record = { id: 'cursor', name: 'Cursor', status: 'ready',
        message: parsed.windows.length ? '' : parsed.unlimited
          ? 'Cursor reports an unlimited plan without metered quota windows.'
          : 'Cursor reports no metered quota windows for this account yet.',
        source: checked.source, updatedAt: now,
        account: { label: checked.account.label, plan: parsed.plan || checked.account.plan },
        headlineId: parsed.headlineId, windows: parsed.windows };
      await cache.put(auth.key, record);
      return record;
    })();
    pending.set(auth.key, operation);
    try { return await operation; } finally { pending.delete(auth.key); }
  }
  return { id: 'cursor', read };
}

module.exports = { createProvider, parseUsage };

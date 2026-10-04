'use strict';

const { TTL_MS, createCache, retryAt } = require('./cursor-runtime');
const { discover, resolveEndpoint, rpcRequest, QUOTA_METHOD, STATUS_METHOD } = require('./antigravity-bridge');
const { parseQuota, parseStatus, headline } = require('./antigravity-quota');

function empty(status, message, account = { label: null, plan: null }) {
  return { id: 'antigravity', name: 'Antigravity', status, message,
    source: 'Antigravity local RPC', updatedAt: null, account, headlineId: null, windows: [] };
}

function discoveryFailure(code) {
  if (code === 'not-running') return empty('unavailable',
    'Start the Antigravity IDE or an existing signed-in agy session, then refresh. Live quota needs its local language server running.');
  if (code === 'unsupported-platform') return empty('unavailable',
    'Antigravity local process discovery supports macOS, Windows, and Linux.');
  if (code === 'missing-command') return empty('unavailable',
    'Antigravity process discovery needs ps on macOS/Linux or PowerShell and netstat on Windows. Make those system tools available and refresh.');
  if (code === 'missing-port-command') return empty('unavailable',
    'Antigravity socket discovery needs lsof on this system. Make lsof available and refresh.');
  if (code === 'no-ports') return empty('unavailable',
    'Antigravity is running but its local server has no readable listening port yet. Wait for it to finish starting and refresh.');
  return empty('unavailable',
    'Antigravity local quota could not be connected. Open its Models and Usage panel, check that you are signed in, then refresh.');
}

function rejected(response) {
  return response.status === 401 || response.status === 403
    || response.body?.code === 'unauthenticated' || response.body?.code === 'permission_denied';
}

function createProvider(options = {}) {
  const cache = createCache(options.storagePath, 'antigravity');
  let lastEndpoint = null;
  let currentKey = null;
  let cooldown = 0;
  // Serialization makes one verified account own each local response. Each queued
  // read re-discovers and verifies its own account instead of reusing an in-flight result.
  let queue = Promise.resolve();

  async function perform({ now = Date.now(), force = false } = {}) {
    if (now < cooldown) return empty('unavailable',
      'Antigravity rate limited this reader. Wait a few minutes before refreshing.');
    let endpoints;
    try { endpoints = await discover(options); }
    catch (error) { lastEndpoint = null; currentKey = null; await cache.clear(); return discoveryFailure(error.code); }
    const reusable = lastEndpoint && endpoints.some(e => e.pid === lastEndpoint.pid
      && e.port === lastEndpoint.port && e.csrfToken === lastEndpoint.csrfToken);
    let endpoint;
    try { endpoint = reusable ? lastEndpoint : await resolveEndpoint(endpoints, options); }
    catch (error) { lastEndpoint = null; await cache.clear(); return discoveryFailure(error.code); }
    lastEndpoint = endpoint;

    let identity;
    let statusResponse;
    try {
      statusResponse = await rpcRequest(endpoint, STATUS_METHOD,
        { metadata: { ideName: 'antigravity' } }, options, true);
      if (statusResponse.status === 429) cooldown = retryAt(statusResponse.retryAfter, now);
      if (rejected(statusResponse)) {
        currentKey = null; await cache.clear();
        return endpoint.kind === 'cli' && !endpoint.csrfToken
          ? empty('unavailable', 'This agy session requires a local token it does not expose. Keep the Antigravity IDE running, then refresh.')
          : empty('needs-auth', 'Antigravity rejected its local session. Sign in again in Antigravity, then refresh.');
      }
      if (statusResponse.body?.userStatus?.isSignedIn === false) {
        currentKey = null; await cache.clear();
        return empty('needs-auth', 'Sign in to Antigravity, then refresh.');
      }
      identity = statusResponse.status === 200 ? parseStatus(statusResponse.body) : null;
    } catch { identity = null; }

    // Never decorate a local server with identity from a different CLI token file.
    // An absent identity cannot qualify an old account's cache for replay.
    if (!identity?.key) { await cache.clear(); currentKey = null; }
    else if (currentKey !== null && currentKey !== identity.key) { await cache.clear(); }
    currentKey = identity?.key || null;
    const saved = identity?.key ? await cache.get(identity.key) : null;
    async function verifyIdentity() {
      if (!identity?.key) return null;
      try {
        const response = await rpcRequest(endpoint, STATUS_METHOD,
          { metadata: { ideName: 'antigravity' } }, options, true);
        return response.status === 200 && !rejected(response) ? parseStatus(response.body) : null;
      } catch { return null; }
    }
    async function staleOrUnavailable(message) {
      const checked = await verifyIdentity();
      if (saved && checked?.key === identity.key) return { ...saved, status: 'stale', message };
      await cache.clear(); currentKey = checked?.key || null;
      return empty('unavailable', 'Antigravity quota or account could not be verified. Keep Antigravity running, sign in if needed, and refresh.', checked?.account);
    }
    if (!force && saved && now >= saved.updatedAt && now - saved.updatedAt < TTL_MS) {
      return { ...saved, account: identity.account };
    }
    if (now < cooldown) return empty('unavailable',
      'Antigravity rate limited this reader. Wait a few minutes before refreshing.', identity?.account);

    let quotaResponse;
    try {
      quotaResponse = await rpcRequest(endpoint, QUOTA_METHOD, { forceRefresh: true }, options, true);
    } catch {
      lastEndpoint = null;
      return staleOrUnavailable('Antigravity quota could not be reached. The last observation is shown; keep Antigravity running and refresh.');
    }
    if (quotaResponse.status === 429) cooldown = retryAt(quotaResponse.retryAfter, now);
    if (rejected(quotaResponse)) {
      await cache.clear(); currentKey = null;
      return empty('needs-auth', 'Antigravity rejected the quota request. Sign in again in Antigravity, then refresh.', identity?.account);
    }
    if (quotaResponse.status === 429) return staleOrUnavailable(
      'Antigravity rate limited this reader. The last observation is shown; retry later.');
    let windows = quotaResponse.status === 200 ? parseQuota(quotaResponse.body) : [];
    let legacy = false;
    if (!windows.length && statusResponse?.status === 200 && identity?.windows.length) {
      // Legacy GetUserStatus only supplies model/session quota. It cannot invent a weekly window.
      windows = identity.windows;
      legacy = true;
    }
    if (!windows.length) return staleOrUnavailable(
      'Antigravity returned no measured quota windows. The last observation is shown; refresh later.');

    // Recheck after the quota call too: a switch mid-request must discard the result.
    if (identity?.key) {
      const verified = await verifyIdentity();
      if (!verified?.key || verified.key !== identity.key) {
        await cache.clear(); currentKey = verified?.key || null;
        return empty('unavailable', 'The Antigravity account could not be verified after refreshing. Sign in if needed and refresh again.');
      }
      identity = verified;
    }
    const record = { id: 'antigravity', name: 'Antigravity', status: 'ready',
      message: legacy ? 'Antigravity supplied legacy model quota. Weekly quota is unavailable from this local response.'
        : identity?.key ? '' : 'The local quota is measured, but this server did not report an account identity or plan.',
      source: legacy ? 'Antigravity local GetUserStatus' : 'Antigravity local RPC',
      updatedAt: now, account: identity?.account || { label: null, plan: null },
      headlineId: headline(windows), windows };
    if (identity?.key) await cache.put(identity.key, record);
    return record;
  }

  function read(args) {
    const task = queue.then(() => perform(args)).catch(() => discoveryFailure('read-failed'));
    queue = task.then(() => undefined);
    return task;
  }
  return { id: 'antigravity', read };
}

module.exports = { createProvider };

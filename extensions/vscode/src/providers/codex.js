'use strict';

const os = require('node:os');
const path = require('node:path');
const { number, clean, opaque, epoch, absoluteRoot, maskedEmail, readJson, cacheFile, writeCache, clearCache } = require('./codex-files');
const { scanLocal } = require('./codex-local');

const BASE = { id: 'codex', name: 'Codex', source: 'Codex local session metadata', headlineId: 'session' };
const FRESH_MS = 5 * 60_000;

function claims(token) {
  if (typeof token !== 'string' || token.length > 128 * 1024) return {};
  try {
    const value = JSON.parse(Buffer.from(token.split('.')[1] || '', 'base64url').toString('utf8'));
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  } catch { return {}; }
}

function planLabel(value) {
  const text = clean(value).toLowerCase();
  return ({ pro: 'Pro', plus: 'Plus', free: 'Free', team: 'Team', business: 'Business', enterprise: 'Enterprise', edu: 'Edu' })[text] || null;
}

async function authentication(root) {
  const raw = await readJson(path.join(root, 'auth.json'));
  if (raw.kind !== 'ok') return { kind: raw.kind };
  const tokens = raw.value.tokens;
  const access = typeof tokens?.access_token === 'string' ? tokens.access_token.trim() : '';
  const apiKey = typeof raw.value.OPENAI_API_KEY === 'string' ? raw.value.OPENAI_API_KEY.trim() : '';
  if (!access && !apiKey) return { kind: 'signed-out' };
  const id = claims(tokens?.id_token);
  const accessClaims = claims(access);
  const info = id['https://api.openai.com/auth'] || accessClaims['https://api.openai.com/auth'] || {};
  const identities = [tokens?.account_id, info.chatgpt_account_id, info.chatgpt_user_id, id.sub, accessClaims.sub]
    .filter(value => typeof value === 'string' && value.length).map(String);
  const mode = access ? 'oauth' : 'api';
  const key = opaque(`${root}\0${mode}\0${identities[0] || access || apiKey}\0${id.email || ''}`);
  return { kind: 'ok', key, identities, mode, modifiedAt: raw.mtime,
    expiresAt: epoch(accessClaims.exp), account: { label: maskedEmail(id.email || accessClaims.email), plan: planLabel(info.chatgpt_plan_type) } };
}

function restoredQuota(value) {
  if (!value || epoch(value.updatedAt) === null || !Array.isArray(value.windows)) return null;
  const windows = value.windows.slice(0, 50).flatMap(item => {
    const id = clean(item?.id);
    const label = clean(item?.label);
    return id && label && number(item.usedPercent) !== null ? [{ id, label, usedPercent: item.usedPercent, resetsAt: epoch(item.resetsAt) }] : [];
  });
  return windows.length ? { windows, updatedAt: epoch(value.updatedAt), plan: planLabel(value.plan) } : null;
}

function createProvider(options = {}) {
  const home = absoluteRoot(options.homeDir, os.homedir());
  const root = home && absoluteRoot(options.paths?.codex, path.join(home, '.codex'));
  const file = root && cacheFile(options.storagePath, root, 'codex-usage-v1.json');
  let loaded = false;
  let state = null;
  let minimumBoundary = 0;
  let inFlight = null;

  async function readOnce(now) {
    if (!root) return { ...BASE, status: 'unavailable', message: 'Codex data path must be an absolute directory.', updatedAt: null, windows: [] };
    if (!loaded) {
      loaded = true;
      const cached = file ? await readJson(file) : null;
      if (cached?.kind === 'ok' && cached.value.version === 1 && cached.value.key === null) {
        minimumBoundary = epoch(cached.value.boundary) || 0;
      } else if (cached?.kind === 'ok' && cached.value.version === 1 && typeof cached.value.key === 'string') state = {
        key: cached.value.key, boundary: epoch(cached.value.boundary), quota: restoredQuota(cached.value.quota),
      };
    }
    const auth = await authentication(root);
    if (auth.kind !== 'ok') {
      state = null;
      minimumBoundary = Math.max(minimumBoundary, now);
      await clearCache(file);
      await writeCache(file, { version: 1, key: null, boundary: minimumBoundary, quota: null });
      return { ...BASE, status: 'needs-auth', message: 'Sign in to Codex to read usage for the current account.', updatedAt: null, windows: [] };
    }
    if (state?.key !== auth.key) {
      const cutoff = state !== null ? now : Math.min(now, auth.modifiedAt || now);
      await clearCache(file);
      // Unlabelled rollouts before the credential file was last written cannot
      // safely be attributed to the new login. The boundary persists across
      // token rotations for a verified account ID.
      state = { key: auth.key, boundary: Math.max(minimumBoundary, cutoff), quota: null };
    }
    if (state.boundary === null) state.boundary = Math.min(now, auth.modifiedAt || now);
    const local = await scanLocal(root, auth, state.boundary, now);
    const after = await authentication(root);
    if (after.kind !== 'ok' || after.key !== auth.key) {
      state = null;
      minimumBoundary = Math.max(minimumBoundary, now);
      await clearCache(file);
      await writeCache(file, { version: 1, key: null, boundary: minimumBoundary, quota: null });
      return { ...BASE, status: 'needs-auth', message: 'Codex sign-in changed. Refresh to read the current account.', updatedAt: null, windows: [] };
    }
    let retained = false;
    if (local.quota && (!state.quota || local.quota.updatedAt >= state.quota.updatedAt)) state.quota = local.quota;
    else if (state.quota) retained = true;
    await writeCache(file, { version: 1, key: state.key, boundary: state.boundary, quota: state.quota });
    const quota = state.quota;
    let status = 'ready';
    const messages = [];
    if (auth.mode === 'api') { status = 'unavailable'; messages.push('API-key sign-in does not provide subscription quota windows in these logs.'); }
    else if (!quota) { status = 'unavailable'; messages.push('No quota observation for this sign-in. Run a Codex turn, then refresh.'); }
    else if (retained || now - quota.updatedAt >= FRESH_MS || quota.updatedAt > now || quota.windows.some(window => window.resetsAt !== null && window.resetsAt <= now)) {
      status = 'stale'; messages.push('Showing a previous local quota observation. Run a Codex turn for a fresh reading.');
    }
    if (auth.expiresAt !== null && auth.expiresAt <= now) {
      status = quota ? 'stale' : 'needs-auth'; messages.push('Codex sign-in has expired. Open Codex to renew it.');
    }
    if (local.identityPartial) messages.push('Local totals are partial: earlier logs cannot be attributed to this sign-in.');
    if (local.partial) messages.push('Local totals and sessions are partial because metadata was unavailable or scan limits applied.');
    return { ...BASE, status, message: messages.join(' '), updatedAt: quota?.updatedAt ?? local.lastUsageAt,
      account: { label: auth.account.label, plan: auth.account.plan || planLabel(quota?.plan) }, windows: quota?.windows || [],
      usage: local.usage, sessions: local.sessions };
  }

  return { id: 'codex', read({ now = Date.now() } = {}) {
    if (inFlight) return inFlight;
    inFlight = readOnce(now).catch(() => ({ ...BASE, status: 'error', message: 'Codex local usage could not be read.', updatedAt: null, windows: [] }))
      .finally(() => { inFlight = null; });
    return inFlight;
  } };
}

module.exports = { createProvider };

'use strict';

const os = require('node:os');
const path = require('node:path');
const childProcess = require('node:child_process');
const { number, clean, opaque, epoch, absoluteRoot, maskedEmail, readJson, cacheFile, writeCache, clearCache } = require('./codex-files');
const { localUsage } = require('./claude-local');
const { boundedJson } = require('./claude-response');

const USAGE_URL = 'https://api.anthropic.com/api/oauth/usage';
const TTL = 5 * 60_000;
const TIMEOUT = 8000;
const BASE = { id: 'claude', name: 'Claude Code', source: 'Claude Code OAuth + local usage', headlineId: 'session' };
const titles = { session: 'Current session', weekly_all: 'All models', weekly_scoped: 'Weekly model limit' };

function usageWindows(payload) {
  const windows = [];
  const ids = new Set();
  function add(id, label, percent, reset) {
    if (number(percent) === null || ids.has(id)) return;
    ids.add(id);
    windows.push({ id, label, usedPercent: percent, resetsAt: epoch(reset) });
  }
  if (Array.isArray(payload?.limits)) for (const limit of payload.limits.slice(0, 50)) {
    if (!limit || typeof limit.kind !== 'string') continue;
    const kind = clean(limit.kind);
    if (!kind) continue;
    const model = clean(limit.scope?.model?.display_name ?? limit.scope?.model?.displayName);
    const id = kind === 'weekly_scoped' && model ? `${kind}-${opaque(model).slice(0, 12)}` : kind;
    add(id, model || titles[kind] || kind.replace(/_/g, ' '), limit.percent, limit.resets_at ?? limit.resetsAt);
  }
  add('session', 'Current session', payload?.five_hour?.utilization, payload?.five_hour?.resets_at);
  add('weekly_all', 'All models', payload?.seven_day?.utilization, payload?.seven_day?.resets_at);
  // Older endpoint shapes name each model bucket explicitly.
  if (payload && typeof payload === 'object') for (const [key, value] of Object.entries(payload)) {
    if (!/^seven_day_[a-z0-9_]+$/i.test(key)) continue;
    const named = clean(value?.scope?.model?.display_name ?? value?.model?.display_name);
    const label = named || key.slice('seven_day_'.length).split('_').map(word => word[0]?.toUpperCase() + word.slice(1)).join(' ');
    if (windows.some(window => window.label.toLowerCase() === label.toLowerCase())) continue;
    add(key, label, value?.utilization, value?.resets_at);
  }
  return windows;
}

function planLabel(credential, account) {
  const tier = clean(account?.organizationRateLimitTier);
  const max = /max_(\d+)x/.exec(tier);
  if (max) return `Max (${max[1]}x)`;
  const type = clean(credential?.subscriptionType || tier).toLowerCase();
  if (type.includes('max')) return 'Max';
  if (type.includes('pro')) return 'Pro';
  if (type.includes('enterprise')) return 'Enterprise';
  if (type.includes('team')) return 'Team';
  return null;
}

function execSecret(execFile, service) {
  return new Promise(resolve => {
    let settled = false;
    const done = (error, stdout) => {
      if (settled) return;
      settled = true;
      if (error) return resolve({ kind: error.code === 44 ? 'missing' : 'error' });
      try {
        const value = JSON.parse(String(stdout));
        resolve(value && typeof value === 'object' ? { kind: 'ok', value } : { kind: 'invalid' });
      } catch { resolve({ kind: 'invalid' }); }
    };
    try {
      execFile('/usr/bin/security', ['find-generic-password', '-s', service, '-w'],
        { timeout: 5000, maxBuffer: 128 * 1024, encoding: 'utf8', windowsHide: true }, done);
    } catch { done(new Error('credential read failed')); }
  });
}

async function credentials(root, home, platform, execFile) {
  // A present file, including an emptied signed-out credential, is authoritative.
  let raw = await readJson(path.join(root, '.credentials.json'));
  if (raw.kind === 'missing' && platform === 'darwin') {
    const services = [`Claude Code-credentials-${opaque(root).slice(0, 8)}`];
    if (root === path.join(home, '.claude')) services.push('Claude Code-credentials');
    for (const service of services) {
      raw = await execSecret(execFile, service);
      if (raw.kind !== 'missing') break;
    }
  }
  if (raw.kind !== 'ok') return { kind: raw.kind };
  const oauth = raw.value.claudeAiOauth;
  if (!oauth || typeof oauth.accessToken !== 'string' || !oauth.accessToken.trim()) return { kind: 'signed-out' };
  const metaFile = root === path.join(home, '.claude') ? path.join(home, '.claude.json') : path.join(root, '.claude.json');
  const meta = await readJson(metaFile);
  const account = meta.kind === 'ok' ? meta.value.oauthAccount : null;
  // Token changes invalidate readings conservatively, even when an opaque token
  // rotates before Claude Code has updated its account metadata.
  const key = opaque(`${root}\0${account?.accountUuid || ''}\0${account?.organizationUuid || ''}\0${account?.emailAddress || ''}\0${oauth.accessToken}`);
  return { kind: 'ok', token: oauth.accessToken, key, expiresAt: epoch(oauth.expiresAt),
    // Keychain reads have no trustworthy item timestamp through this command.
    // Account metadata alone cannot date an opaque token's account switch.
    boundaryHint: raw.mtime ? Math.max(raw.mtime, meta.mtime || 0) : null,
    account: { label: maskedEmail(account?.emailAddress), plan: planLabel(oauth, account) } };
}

function restoredQuota(value) {
  if (!value || epoch(value.updatedAt) === null || !Array.isArray(value.windows)) return null;
  const windows = value.windows.slice(0, 50).flatMap(window => {
    const id = clean(window?.id);
    const label = clean(window?.label);
    return id && label && number(window.usedPercent) !== null
      ? [{ id, label, usedPercent: window.usedPercent, resetsAt: epoch(window.resetsAt) }] : [];
  });
  return { windows, updatedAt: epoch(value.updatedAt) };
}

function retryDelay(response, now) {
  const value = response.headers?.get?.('retry-after');
  const seconds = typeof value === 'string' && /^\d+(\.\d+)?$/.test(value) ? Number(value) : null;
  const date = seconds === null ? epoch(value) : null;
  return Math.min(24 * 60 * 60_000, Math.max(TTL, seconds !== null ? seconds * 1000 : date !== null ? date - now : TTL));
}

function createProvider(options = {}) {
  const home = absoluteRoot(options.homeDir, os.homedir());
  const root = home && absoluteRoot(options.paths?.claude, path.join(home, '.claude'));
  const file = root && cacheFile(options.storagePath, root, 'claude-usage-v1.json');
  const fetchUsage = options.fetch || globalThis.fetch;
  const execFile = options.execFile || childProcess.execFile;
  const platform = options.platform || process.platform;
  let state = null;
  let loaded = false;
  let inFlight = null;

  async function readOnce(now, force) {
    if (!root) return { ...BASE, status: 'unavailable', message: 'Claude data path must be an absolute directory.', updatedAt: null, windows: [] };
    if (!loaded) {
      loaded = true;
      const cache = file ? await readJson(file) : null;
      if (cache?.kind === 'ok' && cache.value.version === 1 && typeof cache.value.key === 'string') {
        state = { key: cache.value.key, boundary: epoch(cache.value.boundary), quota: restoredQuota(cache.value.quota),
          nextTryAt: epoch(cache.value.nextTryAt) || 0, waitReason: clean(cache.value.waitReason) };
      }
    }
    const auth = await credentials(root, home, platform, execFile);
    if (auth.kind !== 'ok') {
      state = null;
      await clearCache(file);
      return { ...BASE, status: 'needs-auth', message: auth.kind === 'error'
        ? 'Claude Code credentials could not be read. Check file or macOS Keychain access.'
        : 'Sign in to Claude Code to read account usage.', updatedAt: null, windows: [] };
    }
    if (state?.key !== auth.key) {
      state = { key: auth.key, boundary: Math.min(now, auth.boundaryHint || now), quota: null, nextTryAt: 0, waitReason: '' };
      await clearCache(file);
    }
    if (state.boundary === null) state.boundary = Math.min(now, auth.boundaryHint || now);
    const localPromise = localUsage(root, now, state.boundary);
    let status = 'ready';
    let message = '';
    const expiredCredential = auth.expiresAt !== null && auth.expiresAt <= now;
    const recent = state.quota && now >= state.quota.updatedAt && now - state.quota.updatedAt < TTL;
    if (expiredCredential) {
      status = state.quota ? 'stale' : 'needs-auth';
      message = 'Claude Code access has expired. Open Claude Code to renew its sign-in.';
    } else if (now < state.nextTryAt) {
      status = state.waitReason === 'auth' ? 'needs-auth' : state.quota ? 'stale' : 'unavailable';
      message = state.waitReason === 'auth' ? 'Claude rejected this sign-in. Open Claude Code and sign in again.'
        : state.waitReason === 'rate' ? 'Claude usage is rate limited. Waiting before retrying.'
          : 'Claude usage could not be fetched. Waiting before retrying.';
    } else if (force || !recent) {
      try {
        // This is the only request. Never exchange or refresh a provider token.
        const signal = AbortSignal.timeout(TIMEOUT);
        const response = await fetchUsage(USAGE_URL, { method: 'GET', redirect: 'error', headers: {
          authorization: `Bearer ${auth.token}`, 'anthropic-beta': 'oauth-2025-04-20', 'content-type': 'application/json',
        }, signal });
        if (response.status === 401 || response.status === 403) {
          state.quota = null;
          state.nextTryAt = now + TTL;
          state.waitReason = 'auth';
          status = 'needs-auth';
          message = 'Claude rejected this sign-in. Open Claude Code and sign in again.';
        } else if (response.status === 429) {
          state.nextTryAt = now + retryDelay(response, now);
          state.waitReason = 'rate';
          status = state.quota ? 'stale' : 'unavailable';
          message = 'Claude usage is rate limited. Waiting before retrying.';
        } else if (!response.ok) {
          state.nextTryAt = now + 60_000;
          state.waitReason = 'network';
          status = state.quota ? 'stale' : 'error';
          message = 'Claude usage could not be fetched. Try again later.';
        } else {
          const payload = await boundedJson(response, signal);
          if (!payload || typeof payload !== 'object' || Array.isArray(payload)) throw new Error('invalid response');
          const windows = usageWindows(payload);
          state.quota = { windows, updatedAt: now };
          state.nextTryAt = 0;
          state.waitReason = '';
          if (!windows.length) { status = 'unavailable'; message = 'Claude reported no metered usage windows for this account.'; }
        }
      } catch {
        state.nextTryAt = now + 60_000;
        state.waitReason = 'network';
        status = state.quota ? 'stale' : 'error';
        message = 'Claude usage could not be fetched. Check your connection and try again.';
      }
      await writeCache(file, { version: 1, key: state.key, boundary: state.boundary, quota: state.quota,
        nextTryAt: state.nextTryAt, waitReason: state.waitReason });
    }
    // Recheck identity after network/local work, so a mid-read sign-out or account
    // switch cannot publish the response obtained with the previous credential.
    const local = await localPromise;
    const after = await credentials(root, home, platform, execFile);
    if (after.kind !== 'ok' || after.key !== auth.key) {
      state = null;
      await clearCache(file);
      return { ...BASE, status: 'needs-auth', message: 'Claude sign-in changed. Refresh to read the current account.', updatedAt: null, windows: [] };
    }
    const quota = state.quota;
    if (status === 'ready' && !quota?.windows.length) { status = 'unavailable'; message = 'Claude reported no metered usage windows for this account.'; }
    if (status === 'ready' && quota.windows.some(window => window.resetsAt !== null && window.resetsAt <= now)) {
      status = 'stale'; message = 'A recorded Claude reset has passed. Waiting for a fresh usage reading.';
    }
    if (local.partial) message = `${message ? `${message} ` : ''}Local token totals and sessions are partial because some metadata was unavailable or scan limits applied.`;
    if (local.identityPartial) message = `${message ? `${message} ` : ''}Earlier local history cannot be attributed across sign-ins. Only records since this connection are included.`;
    return { ...BASE, status, message, account: auth.account, updatedAt: quota?.updatedAt ?? null,
      windows: quota?.windows || [], usage: local.usage, sessions: local.sessions };
  }

  return { id: 'claude', read({ now = Date.now(), force = false } = {}) {
    if (inFlight) return inFlight;
    inFlight = readOnce(now, force).catch(() => ({ ...BASE, status: 'error', message: 'Claude usage could not be read.', updatedAt: null, windows: [] }))
      .finally(() => { inFlight = null; });
    return inFlight;
  } };
}

module.exports = { createProvider };

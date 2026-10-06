'use strict';

// Kimi Code usage, ported from the notch's KimiCredentials/KimiProvider/KimiUsage.
// Reads the OAuth session the Kimi Code CLI writes on sign-in and asks the same
// endpoint the CLI's own `/usage` asks. The access token lives fifteen minutes
// and refreshing it is the CLI's job: this reader never writes, refreshes or
// rotates it. An expired token is a sign-in prompt, not a refresh.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { number, clean, opaque, epoch, absoluteRoot, readJson, cacheFile, writeCache } = require('./codex-files');
const { boundedJson } = require('./claude-response');

const ENDPOINT = 'https://api.kimi.com/coding/v1/usages';
const TTL = 5 * 60_000;
const TIMEOUT = 15_000;
const BASE = { id: 'kimi', name: 'Kimi', source: 'Kimi Code OAuth', headlineId: 'rolling' };
const SIGN_IN = 'Sign in to Kimi Code (run kimi, then /login) to read usage.';

function dataRoot(options = {}) {
  const home = absoluteRoot(options.homeDir, os.homedir());
  const env = options.env || process.env;
  const override = typeof env.KIMI_CODE_HOME === 'string' && env.KIMI_CODE_HOME ? env.KIMI_CODE_HOME : undefined;
  const fallback = override ?? (home ? path.join(home, '.kimi-code') : undefined);
  return absoluteRoot(options.paths?.kimi, fallback);
}

// Counts arrive as decimal strings on the wire; numbers are accepted too.
function count(value) {
  if (number(value) !== null) return Math.trunc(value);
  if (typeof value === 'string' && /^-?\d{1,15}$/.test(value.trim())) return Number(value.trim());
  return null;
}

function claims(token) {
  if (typeof token !== 'string' || token.length > 64 * 1024) return {};
  try {
    const value = JSON.parse(Buffer.from(token.split('.')[1] || '', 'base64url').toString('utf8'));
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  } catch { return {}; }
}

async function credentials(root) {
  const raw = await readJson(path.join(root, 'credentials', 'kimi-code.json'));
  if (raw.kind !== 'ok') return { kind: raw.kind === 'error' ? 'error' : 'signed-out' };
  const token = typeof raw.value.access_token === 'string' ? raw.value.access_token : '';
  if (!token) return { kind: 'signed-out' };
  // `expires_at` is epoch seconds. A file without one is not a session to trust.
  const seconds = number(raw.value.expires_at);
  if (seconds === null || seconds <= 0) return { kind: 'signed-out' };
  // The token rotates every fifteen minutes; key on its subject when it has one
  // so a rotation keeps the reading, while a different account drops it.
  const subject = claims(token);
  const identity = [subject.sub, subject.user_id, subject.userId].find(value => typeof value === 'string' && value) || token;
  return { kind: 'ok', token, expiresAt: seconds * 1000, key: opaque(`${root}\0${identity}`) };
}

function row(id, label, detail) {
  if (!detail || typeof detail !== 'object') return null;
  const limit = count(detail.limit);
  let used = count(detail.used);
  if (used === null) {
    const remaining = count(detail.remaining);
    if (limit !== null && limit > 0 && remaining !== null && remaining >= 0 && remaining <= limit) used = limit - remaining;
    else return null;
  }
  // A count with no limit is not a percentage; never invent one.
  if (limit === null || limit <= 0) return null;
  const usedPercent = Math.min(100, Math.max(0, (used / limit) * 100));
  return { id, label, usedPercent, resetsAt: typeof detail.resetTime === 'string' ? epoch(detail.resetTime) : null };
}

// Only the windows Kimi meters today get named; anything else is left out
// rather than mislabelled. Minute durations divisible by 60 become hours.
function windowKind(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const duration = count(raw.duration);
  const unit = raw.timeUnit;
  if (duration === null || duration <= 0 || typeof unit !== 'string') return null;
  if (unit === 'TIME_UNIT_MINUTE' && duration % 60 === 0) return duration / 60 === 5 ? { id: 'rolling', label: '5h limit' } : null;
  if (unit === 'TIME_UNIT_HOUR' && duration === 5) return { id: 'rolling', label: '5h limit' };
  if (unit === 'TIME_UNIT_WEEK' && duration === 1) return { id: 'weekly', label: 'Weekly limit' };
  return null;
}

// `LEVEL_ADVANCED` -> "Advanced"; the raw level when there is no prefix.
function planLabel(root) {
  const level = clean(root?.user?.membership?.level);
  if (!level) return null;
  const name = level.startsWith('LEVEL_') ? level.slice('LEVEL_'.length) : level;
  return name.toLowerCase().replace(/(^|[^a-z0-9])([a-z])/g, (_, edge, letter) => edge + letter.toUpperCase()) || null;
}

function parseUsage(payload) {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return null;
  const windows = [];
  const add = window => { if (window && !windows.some(item => item.id === window.id)) windows.push(window); };
  // `usage` is the account summary: the weekly window, as the CLI assumes.
  add(row('weekly', 'Weekly limit', payload.usage));
  const limits = Array.isArray(payload.limits) ? payload.limits.slice(0, 50) : [];
  for (const entry of limits) {
    const kind = windowKind(entry?.window);
    if (kind) add(row(kind.id, kind.label, entry.detail));
  }
  return { windows, plan: planLabel(payload) };
}

function restore(value) {
  if (!value || typeof value !== 'object' || typeof value.key !== 'string') return null;
  const windows = Array.isArray(value.windows) ? value.windows.slice(0, 10).flatMap(item => {
    const id = clean(item?.id);
    const label = clean(item?.label);
    return id && label && number(item.usedPercent) !== null ? [{ id, label, usedPercent: item.usedPercent, resetsAt: epoch(item.resetsAt) }] : [];
  }) : [];
  return { key: value.key, reading: windows.length && epoch(value.updatedAt) ? { windows, plan: clean(value.plan) || null, updatedAt: epoch(value.updatedAt) } : null,
    nextTryAt: epoch(value.nextTryAt) || 0, waitReason: clean(value.waitReason) };
}

async function detect(options = {}) {
  const root = dataRoot(options);
  if (!root) return false;
  try { return (await fs.promises.stat(root)).isDirectory(); } catch { return false; }
}

function createProvider(options = {}) {
  const root = dataRoot(options);
  const file = root && cacheFile(options.storagePath, root, 'kimi-usage-v1.json');
  const fetchUsage = options.fetch || globalThis.fetch;
  let state = null;
  let loaded = false;
  let inFlight = null;

  function record(status, message, extra = {}) {
    const reading = extra.reading === undefined ? state?.reading : extra.reading;
    const windows = reading?.windows || [];
    return { ...BASE, status, message, updatedAt: reading?.updatedAt ?? null,
      headlineId: windows.find(item => item.id === 'rolling')?.id || windows.find(item => item.id === 'weekly')?.id || windows[0]?.id || BASE.headlineId,
      account: { label: null, plan: reading?.plan ?? null }, windows };
  }

  async function persist() {
    if (!state) return;
    await writeCache(file, { version: 1, key: state.key, windows: state.reading?.windows || [], plan: state.reading?.plan || null,
      updatedAt: state.reading?.updatedAt || null, nextTryAt: state.nextTryAt, waitReason: state.waitReason });
  }

  async function readOnce(now, force) {
    if (!root) return record('unavailable', 'Kimi Code data path must be an absolute directory.', { reading: null });
    if (!loaded) {
      loaded = true;
      const cached = file ? await readJson(file) : null;
      if (cached?.kind === 'ok' && cached.value.version === 1) state = restore(cached.value);
    }
    const auth = await credentials(root);
    if (auth.kind !== 'ok') {
      state = null;
      return record('needs-auth', auth.kind === 'error' ? 'Kimi Code credentials could not be read. Check file access, or sign in again.' : SIGN_IN, { reading: null });
    }
    if (state?.key !== auth.key) state = { key: auth.key, reading: null, nextTryAt: 0, waitReason: '' };
    if (auth.expiresAt <= now) {
      return state.reading
        ? record('stale', 'Kimi Code sign-in has expired. Open Kimi Code to renew it, then refresh.')
        : record('needs-auth', 'Kimi Code sign-in has expired. Open Kimi Code to renew it, then refresh.');
    }
    const reading = state.reading;
    const recent = reading && now >= reading.updatedAt && now - reading.updatedAt < TTL;
    if (now < state.nextTryAt && !force) {
      if (state.waitReason === 'auth') return record('needs-auth', 'Kimi rejected this sign-in. Open Kimi Code and sign in again.');
      if (state.waitReason === 'plan') return record('unavailable', 'No Kimi Code plan on this account.');
      if (state.waitReason === 'empty') return record('unavailable', 'No Kimi Code usage limits on this account.');
      const text = state.waitReason === 'rate' ? 'Kimi usage is rate limited. Waiting before retrying.' : 'Kimi usage could not be fetched. Waiting before retrying.';
      return record(reading ? 'stale' : state.waitReason === 'rate' ? 'unavailable' : 'error', text);
    }
    if (recent && !force) return finish(now);

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), TIMEOUT);
    try {
      // The only request. Never exchange or refresh a provider token.
      const response = await fetchUsage(ENDPOINT, { method: 'GET', redirect: 'error', signal: controller.signal,
        headers: { authorization: `Bearer ${auth.token}`, accept: 'application/json' } });
      if (response.status === 401 || response.status === 403) {
        state.reading = null; state.nextTryAt = now + TTL; state.waitReason = 'auth';
        await persist();
        return record('needs-auth', 'Kimi rejected this sign-in. Open Kimi Code and sign in again.');
      }
      // The endpoint's own answer for an account without a Kimi Code plan.
      if (response.status === 404) {
        state.reading = null; state.nextTryAt = now + TTL; state.waitReason = 'plan';
        await persist();
        return record('unavailable', 'No Kimi Code plan on this account.');
      }
      if (response.status === 429) {
        state.nextTryAt = now + 60_000; state.waitReason = 'rate';
        await persist();
        return record(state.reading ? 'stale' : 'unavailable', 'Kimi usage is rate limited. Waiting before retrying.');
      }
      if (!(response.status >= 200 && response.status < 300)) throw new Error('bad status');
      const parsed = parseUsage(await boundedJson(response, controller.signal));
      if (!parsed) throw new Error('invalid response');
      if (!parsed.windows.length) {
        state.reading = null; state.nextTryAt = now + TTL; state.waitReason = 'empty';
        await persist();
        return record('unavailable', 'No Kimi Code usage limits on this account.', { reading: { windows: [], plan: parsed.plan, updatedAt: now } });
      }
      state.reading = { windows: parsed.windows, plan: parsed.plan, updatedAt: now };
      state.nextTryAt = 0; state.waitReason = '';
      await persist();
    } catch {
      state.nextTryAt = now + 60_000; state.waitReason = 'network';
      await persist();
      return record(state.reading ? 'stale' : 'error', 'Kimi usage could not be fetched. Check your connection and try again.');
    } finally { clearTimeout(timer); }
    return finish(now);
  }

  async function finish(now) {
    // Recheck the sign-in so a mid-read account switch never publishes the old reading.
    const after = await credentials(root);
    if (after.kind !== 'ok' || after.key !== state?.key) {
      state = null;
      return record('needs-auth', 'Kimi Code sign-in changed. Refresh to read the current account.', { reading: null });
    }
    if (state.reading.windows.some(item => item.resetsAt !== null && item.resetsAt <= now)) {
      return record('stale', 'A Kimi usage window has reset. Refresh for a fresh reading.');
    }
    return record('ready', '');
  }

  return { id: 'kimi', read({ now = Date.now(), force = false } = {}) {
    if (inFlight) return inFlight;
    inFlight = readOnce(now, force).catch(() => ({ ...BASE, status: 'error', message: 'Kimi usage could not be read.', updatedAt: null,
      account: { label: null, plan: null }, windows: [] }))
      .finally(() => { inFlight = null; });
    return inFlight;
  } };
}

module.exports = { createProvider, detect, parseUsage };

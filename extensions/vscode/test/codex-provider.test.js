'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { createProvider } = require('../src/providers/codex');

const NOW = new Date(2026, 9, 5, 12, 0).getTime();
const OLD = NOW - 24 * 60 * 60_000;
const SECRET = 'test-secret-codex-access-token';
const PROMPT = 'PRIVATE_CODEX_PROMPT_MUST_NOT_ESCAPE';
const jwt = payload => `header.${Buffer.from(JSON.stringify(payload)).toString('base64url')}.signature`;

async function fixture(t) {
  const homeDir = await fs.mkdtemp(path.join(os.tmpdir(), 'matra-codex-provider-'));
  t.after(() => fs.rm(homeDir, { recursive: true, force: true }));
  const root = path.join(homeDir, '.codex');
  const storagePath = path.join(homeDir, 'extension-cache');
  await fs.mkdir(root, { recursive: true });
  const options = { homeDir, storagePath, fetch: () => { throw new Error('Codex tests must never fetch'); },
    execFile: () => { throw new Error('Codex tests must never execute a process'); } };
  async function json(file, value, mtime = OLD) {
    await fs.mkdir(path.dirname(file), { recursive: true });
    await fs.writeFile(file, JSON.stringify(value));
    await fs.utimes(file, new Date(mtime), new Date(mtime));
  }
  async function auth(account = 'account-a', mtime = OLD, access = SECRET) {
    await json(path.join(root, 'auth.json'), { tokens: { access_token: access, refresh_token: 'do-not-use-refresh', account_id: account,
      id_token: jwt({ email: `${account}@example.com`, sub: `user-${account}`,
        'https://api.openai.com/auth': { chatgpt_account_id: account, chatgpt_user_id: `user-${account}`, chatgpt_plan_type: 'pro' } }) } }, mtime);
  }
  async function rollout(name, rows, archive = false) {
    const file = path.join(root, archive ? 'archived_sessions' : 'sessions', '2026', '10', '05', name);
    await fs.mkdir(path.dirname(file), { recursive: true });
    await fs.writeFile(file, rows.map(row => typeof row === 'string' ? row : JSON.stringify(row)).join('\n') + '\n');
    return file;
  }
  return { homeDir, root, storagePath, options, json, auth, rollout, provider: () => createProvider(options) };
}

const header = (id = 'session-a', at = NOW - 3_600_000, account = null) => ({ type: 'session_meta', timestamp: new Date(at).toISOString(),
  payload: { id, cwd: '/private/work/Matra', ...(account ? { account_id: account } : {}) } });
const context = () => ({ type: 'turn_context', payload: { model: 'gpt-6.1-sol' } });
const limits = (percent = 12, reset = (NOW + 3_600_000) / 1000) => ({ primary: { used_percent: percent, window_minutes: 300, resets_at: reset },
  secondary: { used_percent: 34.5, window_minutes: 10080, resets_at: NOW + 86400_000 }, plan_type: 'pro' });
function event(at = NOW - 60_000, total = 150, rate = limits(), input = 100, output = 50, cached = 20) {
  return { type: 'event_msg', timestamp: new Date(at).toISOString(), payload: { type: 'token_count', rate_limits: rate,
    info: { total_token_usage: { input_tokens: input, output_tokens: output, cached_input_tokens: cached,
      reasoning_output_tokens: 10, total_tokens: total },
    last_token_usage: { input_tokens: 100, output_tokens: 50, cached_input_tokens: 20, total_tokens: 150 } } } };
}

test('Codex requires current account credentials before displaying account-bound local records', async t => {
  const f = await fixture(t);
  await f.rollout('old.jsonl', [header(), event()]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'needs-auth');
  assert.deepEqual(record.windows, []);
  assert.equal(record.updatedAt, null);
  assert.equal(record.usage, undefined);
});

test('Codex reads local quota and today usage without network, subprocess or double-counted subsets', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('session.jsonl', [header(), context(), { type: 'response_item', payload: { content: PROMPT } }, event()]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(record.updatedAt, NOW - 60_000);
  assert.equal(record.windows[0].label, '5h limit');
  assert.equal(record.windows[0].resetsAt, NOW + 3_600_000);
  assert.equal(record.windows[1].resetsAt, NOW + 86400_000);
  assert.deepEqual(record.usage, { period: 'Today (local device)', inputTokens: 100, outputTokens: 50, cachedTokens: 20, totalTokens: 150, costUsd: null });
  assert.equal(record.sessions[0].label, 'Matra');
  assert.equal(record.sessions[0].detail, 'gpt-6.1-sol');
  assert.equal(record.sessions[0].state, 'recent');
  assert.equal(record.account.label, 'a***@example.com');
  assert.equal(record.account.plan, 'Pro');
  for (const forbidden of [SECRET, PROMPT, 'do-not-use-refresh', '/private/work/', 'session-a']) assert.equal(JSON.stringify(record).includes(forbidden), false);
  const cache = await fs.readFile(path.join(f.storagePath, 'codex-usage-v1.json'), 'utf8');
  assert.equal(cache.includes(SECRET), false);
  assert.equal(cache.includes(PROMPT), false);
});

test('Codex repeated rate-limit token_count events and archived copies do not repeat last_token_usage', async t => {
  const f = await fixture(t);
  await f.auth();
  const rows = [header(), event(NOW - 180_000), event(NOW - 120_000), event(NOW - 60_000, 210, limits(19), 140, 70, 30),
    event(NOW - 30_000, 210, limits(20), 140, 70, 30)];
  await f.rollout('live.jsonl', rows);
  await f.rollout('archived.jsonl', rows, true);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.totalTokens, 210);
  assert.equal(record.usage.inputTokens, 140);
  assert.equal(record.usage.cachedTokens, 30);
  assert.equal(record.windows[0].usedPercent, 20);
  assert.equal(record.sessions.length, 1);
});

test('Codex today deltas subtract a previous-day cumulative baseline', async t => {
  const f = await fixture(t);
  await f.auth();
  const midnight = new Date(NOW); midnight.setHours(0, 0, 0, 0);
  await f.rollout('overnight.jsonl', [header('session-a', midnight.getTime() - 3_600_000),
    event(midnight.getTime() - 1000, 150), event(NOW - 60_000, 210, limits(), 140, 70, 30)]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.totalTokens, 60);
  assert.equal(record.usage.inputTokens, 40);
  assert.equal(record.usage.outputTokens, 20);
  assert.equal(record.usage.cachedTokens, 10);
});

test('Codex counter corrections do not recount an earlier cumulative high-water mark', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('reset.jsonl', [header(), event(NOW - 240_000, 150), event(NOW - 180_000, 120, limits(), 80, 40, 10),
    event(NOW - 120_000, 150), event(NOW - 60_000, 180, limits(), 120, 60, 25)]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.totalTokens, 180);
  assert.match(record.usage.period, /partial/);
});

test('Codex an older session without a baseline never charges its entire cumulative total to today', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('old.jsonl', [header('session-a', NOW - 2 * 86400_000), event()]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage, null);
  assert.equal(record.sessions[0].tokens, null);
  assert.match(record.message, /partial/);
});

test('Codex stale observations and expired resets keep real percentages and observation timestamps', async t => {
  const f = await fixture(t);
  await f.auth();
  const raw = limits(175, (NOW - 3_600_000) / 1000);
  await f.rollout('old.jsonl', [header(), event(NOW - 10 * 60_000, 150, raw)]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'stale');
  assert.equal(record.updatedAt, NOW - 10 * 60_000);
  assert.equal(record.windows[0].usedPercent, 175);
  assert.equal(record.windows[0].resetsAt, NOW - 3_600_000);
  assert.equal(record.windows.length, 2);
});

test('Codex unknown percentages, numeric strings and malformed rows do not become zero or discard siblings', async t => {
  const f = await fixture(t);
  await f.auth();
  const raw = limits(); raw.primary.used_percent = null;
  await f.rollout('partial.jsonl', [header(), '{bad-json', event(NOW - 120_000, 150, raw),
    event(NOW - 60_000, 150, { primary: { used_percent: '50' }, secondary: { used_percent: 42, resets_at: 'bad-date' } })]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.windows.length, 1);
  assert.equal(record.windows[0].usedPercent, 42);
  assert.equal(record.windows[0].resetsAt, null);
});

test('Codex multiple limit buckets remain separate and relative resets use the observation time', async t => {
  const f = await fixture(t);
  await f.auth();
  const spark = { limit_id: 'GPT-Codex-Spark', primary: { used_percent: 99, window_minutes: 300, reset_after_seconds: 3600 } };
  await f.rollout('buckets.jsonl', [header(), event(NOW - 120_000), event(NOW - 60_000, 150, spark)]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.windows.length, 3);
  assert.equal(new Set(record.windows.map(w => w.id)).size, 3);
  assert.equal(record.windows.find(w => w.label.startsWith('GPT-Codex-Spark')).resetsAt, NOW - 60_000 + 3_600_000);
});

test('Codex first connection excludes earlier unbound quota and local history', async t => {
  const f = await fixture(t);
  await f.auth('account-a', NOW - 30_000);
  await f.rollout('old-account.jsonl', [header(), event(NOW - 60_000)]);
  const record = await f.provider().read({ now: NOW });
  assert.deepEqual(record.windows, []);
  assert.equal(record.usage, null);
  assert.deepEqual(record.sessions, []);
  assert.match(record.message, /earlier logs/);
});

test('Codex account switch clears persisted quota and does not reload older unbound logs', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('session.jsonl', [header(), event()]);
  const provider = f.provider();
  await provider.read({ now: NOW });
  await f.auth('account-b', NOW + 1000);
  const record = await provider.read({ now: NOW + 2000 });
  assert.deepEqual(record.windows, []);
  assert.equal(record.updatedAt, null);
  assert.equal(record.usage, null);
  assert.deepEqual(record.sessions, []);
});

test('Codex explicit account IDs permit matching historical rows and reject foreign-account rows', async t => {
  const f = await fixture(t);
  await f.auth('account-a', NOW - 30_000);
  await f.rollout('ours.jsonl', [header('ours', NOW - 3_600_000, 'account-a'), event()]);
  await f.rollout('other.jsonl', [header('other', NOW - 3_600_000, 'account-b'), event(NOW - 1000, 1000, limits(90), 800, 200, 10)]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.windows[0].usedPercent, 12);
  assert.equal(record.sessions.length, 1);
});

test('Codex credential rotation for the same verified account preserves its persisted boundary', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('session.jsonl', [header(), event()]);
  const provider = f.provider();
  await provider.read({ now: NOW });
  await f.auth('account-a', NOW + 1000, 'rotated-test-access');
  const restarted = await f.provider().read({ now: NOW + 2000 });
  assert.equal(restarted.windows[0].usedPercent, 12);
  assert.equal(restarted.usage.totalTokens, 150);
});

test('Codex sign-out clears cache and summaries without changing provider files', async t => {
  const f = await fixture(t);
  await f.auth();
  const file = await f.rollout('session.jsonl', [header(), event()]);
  const before = await fs.readFile(file, 'utf8');
  const provider = f.provider();
  await provider.read({ now: NOW });
  await fs.unlink(path.join(f.root, 'auth.json'));
  const record = await provider.read({ now: NOW + 1000 });
  assert.equal(record.status, 'needs-auth');
  assert.deepEqual(record.windows, []);
  assert.equal(record.usage, undefined);
  assert.equal(await fs.readFile(file, 'utf8'), before);
  await assert.rejects(fs.access(path.join(f.storagePath, 'codex-usage-v1.json')));
});

test('Codex unavailable local files retain cached quota as stale without advancing updatedAt', async t => {
  const f = await fixture(t);
  await f.auth();
  const file = await f.rollout('session.jsonl', [header(), event()]);
  const provider = f.provider();
  await provider.read({ now: NOW });
  await fs.unlink(file);
  const record = await provider.read({ now: NOW + 1000 });
  assert.equal(record.status, 'stale');
  assert.equal(record.updatedAt, NOW - 60_000);
  assert.equal(record.windows[0].usedPercent, 12);
});

test('Codex API-key mode never shows an earlier subscription quota', async t => {
  const f = await fixture(t);
  await f.json(path.join(f.root, 'auth.json'), { OPENAI_API_KEY: 'fake-api-key' });
  await f.rollout('session.jsonl', [header(), event()]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'unavailable');
  assert.deepEqual(record.windows, []);
  assert.equal(record.usage.totalTokens, 150);
});

test('Codex expired auth only reports stale observations and never refreshes credentials', async t => {
  const f = await fixture(t);
  await f.auth('account-a', OLD, jwt({ exp: (NOW - 1000) / 1000 }));
  await f.rollout('session.jsonl', [header(), event()]);
  const before = await fs.readFile(path.join(f.root, 'auth.json'), 'utf8');
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'stale');
  assert.match(record.message, /expired/);
  assert.equal(await fs.readFile(path.join(f.root, 'auth.json'), 'utf8'), before);
});

test('Codex incomplete cumulative components remain unknown while total usage is retained', async t => {
  const f = await fixture(t);
  await f.auth();
  const row = event(); delete row.payload.info.total_token_usage.cached_input_tokens;
  await f.rollout('partial.jsonl', [header(), row]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.totalTokens, 150);
  assert.equal(record.usage.cachedTokens, null);
  assert.equal(record.usage.costUsd, null);
});

test('Codex long tail without a baseline returns unknown totals with a partial notice', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('large.jsonl', [header(), '{"type":"response_item","payload":{"content":"' + 'x'.repeat(1024 * 1024 + 100) + '"}}', event()]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage, null);
  assert.equal(record.windows[0].usedPercent, 12);
  assert.equal(record.sessions[0].label, 'Matra');
  assert.match(record.message, /scan limits/);
});

test('Codex explicit absolute directory root works and relative roots are rejected', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.rollout('session.jsonl', [header(), event()]);
  const alternate = path.join(f.homeDir, 'work-codex');
  await fs.rename(f.root, alternate);
  const record = await createProvider({ ...f.options, paths: { codex: alternate } }).read({ now: NOW });
  assert.equal(record.windows[0].usedPercent, 12);
  assert.equal((await createProvider({ ...f.options, paths: { codex: '../relative' } }).read({ now: NOW })).status, 'unavailable');
});

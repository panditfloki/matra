'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { createProvider } = require('../src/providers/claude');

const NOW = new Date(2026, 9, 5, 12, 0).getTime();
const OLD = NOW - 24 * 60 * 60_000;
const SECRET = 'test-secret-claude-token';
const PROMPT = 'PRIVATE_PROMPT_MUST_NOT_ESCAPE';
const response = (body, status = 200, retry = null) => ({ ok: status >= 200 && status < 300, status,
  headers: { get: () => retry }, json: async () => body });
const defaultUsage = () => ({ limits: [{ kind: 'session', percent: 12.5, resets_at: new Date(NOW + 3_600_000).toISOString() },
  { kind: 'weekly_all', percent: 34, resets_at: (NOW + 7 * 86400_000) / 1000 },
  { kind: 'weekly_scoped', percent: 56, scope: { model: { display_name: 'Fable 5.1' } }, resets_at: NOW + 86400_000 }] });

async function fixture(t, extra = {}) {
  const homeDir = await fs.mkdtemp(path.join(os.tmpdir(), 'matra-claude-provider-'));
  t.after(() => fs.rm(homeDir, { recursive: true, force: true }));
  const root = path.join(homeDir, '.claude');
  const storagePath = path.join(homeDir, 'extension-cache');
  await fs.mkdir(root, { recursive: true });
  const options = { homeDir, storagePath, platform: 'linux', fetch: async () => response(defaultUsage()),
    execFile: () => { throw new Error('unit tests must never execute a process'); }, ...extra };
  async function json(file, value, modifiedAt = OLD) {
    await fs.mkdir(path.dirname(file), { recursive: true });
    await fs.writeFile(file, JSON.stringify(value));
    await fs.utimes(file, new Date(modifiedAt), new Date(modifiedAt));
  }
  async function auth(token = SECRET, modifiedAt = OLD, account = 'account-a', expiry = NOW + 3_600_000) {
    await json(path.join(root, '.credentials.json'), { claudeAiOauth: { accessToken: token, refreshToken: 'do-not-use-refresh', expiresAt: expiry, subscriptionType: 'max' } }, modifiedAt);
    await json(path.join(homeDir, '.claude.json'), { oauthAccount: { accountUuid: account, organizationUuid: 'org-a', emailAddress: `${account}@example.com`, organizationRateLimitTier: 'default_claude_max_5x' } }, modifiedAt);
  }
  async function transcript(name, rows) {
    const file = path.join(root, 'projects', 'project', name);
    await fs.mkdir(path.dirname(file), { recursive: true });
    await fs.writeFile(file, rows.map(row => typeof row === 'string' ? row : JSON.stringify(row)).join('\n') + '\n');
    return file;
  }
  return { homeDir, root, storagePath, options, json, auth, transcript, provider: () => createProvider(options) };
}

function assistant({ id = 'message-a', requestId = 'request-a', session = 'session-a', at = NOW - 60_000,
  input = 10, output = 20, read = 5, written = 3, model = 'claude-fable-5.1', cwd = '/private/projects/Matra' } = {}) {
  return { type: 'assistant', timestamp: new Date(at).toISOString(), sessionId: session, requestId, cwd,
    message: { id, model, content: [{ type: 'text', text: PROMPT }],
      usage: { input_tokens: input, output_tokens: output, cache_read_input_tokens: read, cache_creation_input_tokens: written } } };
}

test('Claude has no quota and does not fetch without credentials', async t => {
  let calls = 0;
  const f = await fixture(t, { fetch: async () => { calls++; throw new Error('unexpected request'); } });
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'needs-auth');
  assert.equal(record.updatedAt, null);
  assert.deepEqual(record.windows, []);
  assert.equal(calls, 0);
  assert.equal(record.usage, undefined);
});

test('Claude supports scoped Fable, all-model, legacy and independent unknown buckets', async t => {
  const body = defaultUsage();
  body.limits.push({ kind: 'weekly_scoped', percent: 150, scope: { model: { display_name: 'Opus 5' } }, resets_at: null });
  body.limits.push({ kind: 'unknown', percent: null }, { kind: 'bad-number', percent: '45' });
  body.five_hour = { utilization: 99, resets_at: NOW + 3_600_000 };
  body.seven_day_fable = { utilization: 56, resets_at: NOW + 86400_000, model: { display_name: 'Fable 5.1' } };
  body.seven_day_sonnet = { utilization: 0, resets_at: NOW + 86400_000 };
  const f = await fixture(t, { fetch: async (url, options) => {
    assert.equal(url, 'https://api.anthropic.com/api/oauth/usage');
    assert.equal(options.method, 'GET');
    assert.equal(options.headers.authorization, `Bearer ${SECRET}`);
    return response(body);
  } });
  await f.auth();
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(record.windows.length, 5);
  assert.equal(record.windows.find(w => w.id === 'session').usedPercent, 12.5);
  assert.equal(record.windows.find(w => w.label === 'Fable 5.1').resetsAt, NOW + 86400_000);
  assert.equal(record.windows.find(w => w.label === 'Opus 5').usedPercent, 150);
  assert.equal(record.windows.find(w => w.label === 'Opus 5').resetsAt, null);
  assert.equal(record.windows.find(w => w.label === 'Sonnet').usedPercent, 0);
  assert.equal(new Set(record.windows.map(w => w.id)).size, record.windows.length);
  assert.equal(record.account.plan, 'Max (5x)');
  assert.equal(record.account.label, 'a***@example.com');
  assert.equal(record.usage, null);
});

test('Claude legacy windows survive absent or malformed siblings and epoch units', async t => {
  const f = await fixture(t, { fetch: async () => response({ five_hour: { utilization: null },
    seven_day: { utilization: 23.2, resets_at: String((NOW + 86400_000) / 1000) },
    seven_day_fable: { utilization: 9, resets_at: 'not-a-date' } }) });
  await f.auth();
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.windows.length, 2);
  assert.equal(record.windows[0].resetsAt, NOW + 86400_000);
  assert.equal(record.windows[1].resetsAt, null);
});

test('Claude never invents quota zero from unknown or unmetered payload', async t => {
  const f = await fixture(t, { fetch: async () => response({ limits: [{ kind: 'session', percent: null }] }) });
  await f.auth();
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'unavailable');
  assert.deepEqual(record.windows, []);
  assert.equal(record.updatedAt, NOW);
});

test('Claude local today totals dedupe streaming and fork copies without exposing prompts or full paths', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.transcript('main.jsonl', [assistant({ output: 5 }), assistant({ output: 20 }),
    assistant({ id: 'message-b', requestId: 'request-b', input: 2, output: 4, read: 0, written: 0 })]);
  await f.transcript('fork.jsonl', [assistant({ output: 20 })]);
  const record = await f.provider().read({ now: NOW });
  assert.deepEqual(record.usage, { period: 'Today (local device)', inputTokens: 12, outputTokens: 24, cachedTokens: 8, totalTokens: 44, costUsd: null });
  assert.equal(record.sessions.length, 1);
  assert.equal(record.sessions[0].label, 'Matra');
  assert.equal(record.sessions[0].state, 'recent');
  const serialized = JSON.stringify(record);
  for (const forbidden of [SECRET, 'do-not-use-refresh', PROMPT, '/private/projects/', 'session-a', 'message-a']) assert.equal(serialized.includes(forbidden), false);
  const cache = await fs.readFile(path.join(f.storagePath, 'claude-usage-v1.json'), 'utf8');
  assert.equal(cache.includes(SECRET), false);
  assert.equal(cache.includes(PROMPT), false);
});

test('Claude incomplete token components remain unknown and malformed or synthetic rows are ignored', async t => {
  const f = await fixture(t);
  await f.auth();
  const row = assistant();
  delete row.message.usage.cache_read_input_tokens;
  await f.transcript('session.jsonl', ['{not-json', assistant({ id: 'synthetic', model: '<synthetic>' }), row]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.inputTokens, 10);
  assert.equal(record.usage.cachedTokens, null);
  assert.equal(record.usage.totalTokens, null);
  assert.equal(record.usage.costUsd, null);
  assert.match(record.usage.period, /partial/);
});

test('Claude 429 retains timestamps, respects forced backoff and persists extension-owned cache', async t => {
  let calls = 0;
  const f = await fixture(t, { fetch: async () => response(calls++ === 0 ? defaultUsage() : {}, calls === 1 ? 200 : 429, '600') });
  await f.auth();
  const provider = f.provider();
  await provider.read({ now: NOW });
  const stale = await provider.read({ now: NOW + 360_000, force: true });
  assert.equal(stale.status, 'stale');
  assert.equal(stale.updatedAt, NOW);
  assert.equal(stale.windows[0].usedPercent, 12.5);
  await provider.read({ now: NOW + 361_000, force: true });
  const restored = await f.provider().read({ now: NOW + 362_000, force: true });
  assert.equal(restored.updatedAt, NOW);
  assert.equal(restored.status, 'stale');
  assert.equal(calls, 2);
});

test('Claude transient fetch errors preserve the true observation time and sanitize errors', async t => {
  let first = true;
  const f = await fixture(t, { fetch: async () => { if (first) { first = false; return response(defaultUsage()); } throw new Error(`${SECRET} ${PROMPT}`); } });
  await f.auth();
  const provider = f.provider();
  await provider.read({ now: NOW });
  const record = await provider.read({ now: NOW + 360_000 });
  assert.equal(record.status, 'stale');
  assert.equal(record.updatedAt, NOW);
  assert.equal(JSON.stringify(record).includes(SECRET), false);
});

for (const code of [401, 403]) test(`Claude ${code} discards account quota and preserves needs-auth during backoff`, async t => {
  let first = true;
  const f = await fixture(t, { fetch: async () => { if (first) { first = false; return response(defaultUsage()); } return response({}, code); } });
  await f.auth();
  const provider = f.provider();
  await provider.read({ now: NOW });
  const record = await provider.read({ now: NOW + 360_000 });
  assert.equal(record.status, 'needs-auth');
  assert.deepEqual(record.windows, []);
  assert.equal(record.updatedAt, null);
  assert.equal((await provider.read({ now: NOW + 361_000 })).status, 'needs-auth');
});

test('Claude account switch cannot retain old cache or old local history', async t => {
  let first = true;
  const f = await fixture(t, { fetch: async () => { if (first) { first = false; return response(defaultUsage()); } return response({}, 429); } });
  await f.auth();
  await f.transcript('session.jsonl', [assistant()]);
  const provider = f.provider();
  await provider.read({ now: NOW });
  await f.auth('test-secret-account-b', NOW + 1000, 'account-b');
  const record = await provider.read({ now: NOW + 2000, force: true });
  assert.deepEqual(record.windows, []);
  assert.equal(record.updatedAt, null);
  assert.equal(record.usage, null);
  assert.deepEqual(record.sessions, []);
});

test('Claude sign-out clears persisted observations', async t => {
  const f = await fixture(t);
  await f.auth();
  const provider = f.provider();
  await provider.read({ now: NOW });
  await f.auth('', NOW + 1000);
  const record = await provider.read({ now: NOW + 2000 });
  assert.equal(record.status, 'needs-auth');
  assert.deepEqual(record.windows, []);
  await assert.rejects(fs.access(path.join(f.storagePath, 'claude-usage-v1.json')));
});

test('Claude mid-request account change discards the response', async t => {
  let release;
  const f = await fixture(t, { fetch: async () => { await new Promise(resolve => { release = resolve; }); return response(defaultUsage()); } });
  await f.auth();
  const provider = f.provider();
  const reading = provider.read({ now: NOW });
  while (!release) await new Promise(resolve => setImmediate(resolve));
  await f.auth('new-test-token', NOW, 'account-b');
  release();
  const record = await reading;
  assert.equal(record.status, 'needs-auth');
  assert.deepEqual(record.windows, []);
});

test('Claude expired credentials are not refreshed or sent to the usage endpoint', async t => {
  let calls = 0;
  const f = await fixture(t, { fetch: async () => { calls++; return response(defaultUsage()); } });
  await f.auth(SECRET, OLD, 'account-a', NOW - 1);
  const before = await fs.readFile(path.join(f.root, '.credentials.json'), 'utf8');
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'needs-auth');
  assert.equal(calls, 0);
  assert.equal(await fs.readFile(path.join(f.root, '.credentials.json'), 'utf8'), before);
});

test('Claude expired reset retains its observed percentage', async t => {
  const f = await fixture(t, { fetch: async () => response({ limits: [{ kind: 'session', percent: 73, resets_at: (NOW - 1) / 1000 }] }) });
  await f.auth();
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'stale');
  assert.equal(record.windows[0].usedPercent, 73);
  assert.equal(record.windows[0].resetsAt, NOW - 1);
});

test('Claude reads the macOS profile Keychain with fixed bounded argv only', async t => {
  const calls = [];
  const f = await fixture(t, { platform: 'darwin', execFile: (file, args, options, callback) => {
    calls.push({ file, args, options });
    callback(null, JSON.stringify({ claudeAiOauth: { accessToken: SECRET, expiresAt: NOW + 3_600_000 } }));
  } });
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(calls.length, 2);
  for (const call of calls) {
    assert.equal(call.file, '/usr/bin/security');
    assert.deepEqual(call.args.slice(0, 3), ['find-generic-password', '-s', call.args[2]]);
    assert.match(call.args[2], /^Claude Code-credentials-[a-f0-9]{8}$/);
    assert.equal(call.args[3], '-w');
    assert.equal(call.options.timeout, 5000);
    assert.equal(call.options.maxBuffer, 128 * 1024);
  }
  assert.match(record.message, /since this connection/);
});

test('Claude default Keychain falls back to legacy service only when profile item is absent', async t => {
  const names = [];
  const f = await fixture(t, { platform: 'darwin', execFile: (file, args, options, callback) => {
    names.push(args[2]);
    if (args[2] !== 'Claude Code-credentials') callback(Object.assign(new Error('missing'), { code: 44 }));
    else callback(null, JSON.stringify({ claudeAiOauth: { accessToken: SECRET } }));
  } });
  assert.equal((await f.provider().read({ now: NOW })).status, 'ready');
  assert.equal(names.filter(name => name === 'Claude Code-credentials').length, 2);
});

test('Claude explicit paths override defaults and relative paths are rejected', async t => {
  const f = await fixture(t);
  await f.auth();
  const alternate = path.join(f.homeDir, 'work-claude');
  await f.json(path.join(alternate, '.credentials.json'), { claudeAiOauth: { accessToken: SECRET } });
  assert.equal((await createProvider({ ...f.options, paths: { claude: alternate } }).read({ now: NOW })).status, 'ready');
  const bad = await createProvider({ ...f.options, paths: { claude: './relative' } }).read({ now: NOW });
  assert.equal(bad.status, 'unavailable');
});

test('Claude truncated 1MB tails are explicitly partial and omit transcript contents', async t => {
  const f = await fixture(t);
  await f.auth();
  await f.transcript('large.jsonl', ['{"type":"user","message":{"content":"' + 'x'.repeat(1024 * 1024 + 100) + '"}}', assistant()]);
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.totalTokens, 38);
  assert.match(record.usage.period, /partial/);
  assert.match(record.message, /scan limits/);
});

test('Claude scan caps 500 recent files and 20 session summaries', async t => {
  const f = await fixture(t);
  await f.auth();
  await Promise.all(Array.from({ length: 501 }, (_, index) => f.transcript(`${index}.jsonl`, [assistant({ id: `m-${index}`, requestId: `r-${index}`, session: `s-${index}`, input: 1, output: 0, read: 0, written: 0 })])));
  const record = await f.provider().read({ now: NOW });
  assert.equal(record.usage.totalTokens, 500);
  assert.equal(record.sessions.length, 20);
  assert.match(record.usage.period, /partial/);
});

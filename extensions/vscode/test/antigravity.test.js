'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { createProvider } = require('../src/providers/antigravity');
const { parseQuota, parseStatus, headline } = require('../src/providers/antigravity-quota');
const { discover, parseProcesses, parseLsof, parseNetstat, isRpcProof,
  rpcRequest, QUOTA_METHOD, STATUS_METHOD } = require('../src/providers/antigravity-bridge');
const { TTL_MS, MAX_BODY } = require('../src/providers/cursor-runtime');

const NOW = Date.UTC(2026, 9, 5, 12);
const RESET = '2026-10-05T17:00:00.123Z';
const WEEK = '2026-10-12T12:00:00Z';
const CSRF = 'fixture-csrf-only';
const COMMAND = `/Applications/Antigravity.app/Contents/Resources/app/extensions/antigravity/bin/language_server_macos_arm --csrf_token ${CSRF}`;
const PROCESS_TABLE = ` 41 /Applications/Windsurf.app/bin/language_server_macos_arm --csrf_token unrelated-fixture\n 42 ${COMMAND}\n 43 node --payload language_server --csrf_token decoy-fixture\n`;
const LSOF = 'COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\n'
  + 'language_ 42 fixture 10u IPv4 1 0t0 TCP 127.0.0.1:4317 (LISTEN)\n'
  + 'language_ 42 fixture 11u IPv4 2 0t0 TCP *:5555 (LISTEN)\n'
  + 'unrelated 77 fixture 12u IPv4 3 0t0 TCP 127.0.0.1:6666 (LISTEN)\n';
const quota = (left = 0.8) => ({ response: { groups: [
  { displayName: 'Gemini Models', buckets: [
    { bucketId: 'session', window: '5h', remainingFraction: left, resetTime: RESET },
    { bucketId: 'weekly', window: 'weekly', remaining: { case: 'remainingFraction', value: 0.5 }, resetTime: WEEK }
  ] },
  { displayName: 'Claude and GPT models', buckets: [
    { bucketId: 'session', window: '5h', remainingFraction: 1, resetTime: RESET },
    { bucketId: 'weekly', window: 'weekly', remainingFraction: 0.9, resetTime: WEEK }
  ] }
] } });
const status = (email = 'owner@example.com', tier = 'Google AI Ultra') => ({ userStatus: {
  email, userTier: { name: tier }, planStatus: { planInfo: { planName: 'Google AI Pro' } }
} });
const response = (body, statusCode = 200, headers = {}) => new Response(JSON.stringify(body), {
  status: statusCode, headers: { 'content-type': 'application/json', ...headers }
});
const challenge = () => response({ code: 'unauthenticated', message: 'missing CSRF token' }, 401);

async function fixture(t) {
  const homeDir = await fs.mkdtemp(path.join(__dirname, 'antigravity-fixtures-'));
  t.after(() => fs.rm(homeDir, { recursive: true, force: true }));
  const state = { processes: PROCESS_TABLE, email: 'owner@example.com', tier: 'Google AI Ultra',
    left: 0.8, quotaStatus: 200, quotaError: false, statusStatus: 200, unidentified: false,
    afterQuota: null, legacy: null, signedOut: false };
  const calls = [];
  const processes = [];
  const options = { homeDir, storagePath: path.join(homeDir, 'cache'), platform: 'darwin',
    paths: { rpcUrl: 'https://must-never-be-used.invalid' },
    execFile: async (file, args, settings) => {
      processes.push({ file, args, settings });
      assert.equal(settings.shell, false);
      assert.equal(settings.maxBuffer, MAX_BODY);
      if (file === '/bin/ps') return { stdout: state.processes };
      if (file === '/usr/sbin/lsof') {
        assert.ok(args.includes('-a'));
        assert.equal(args[args.indexOf('-p') + 1], '42');
        return { stdout: LSOF };
      }
      throw new Error('unexpected executable fixture');
    },
    fetch: async (url, init) => {
      const parsed = new URL(url);
      assert.equal(parsed.hostname, '127.0.0.1');
      assert.ok([QUOTA_METHOD, STATUS_METHOD].includes(parsed.pathname));
      const token = init.headers['X-Codeium-Csrf-Token'];
      calls.push({ port: Number(parsed.port), method: parsed.pathname, token, body: init.body });
      if (parsed.port === '4317') {
        assert.equal(token, undefined, 'forwarded app must never receive CSRF');
        return response({ healthy: true, unrelated: 'JSON web app fixture' });
      }
      assert.equal(parsed.port, '5555');
      if (!token) return challenge();
      assert.equal(token, CSRF);
      if (parsed.pathname === STATUS_METHOD) {
        if (state.statusStatus !== 200) return response({ code: 'unavailable' }, state.statusStatus);
        if (state.signedOut) return response({ userStatus: { ...status(state.email, state.tier).userStatus, isSignedIn: false } });
        if (state.unidentified) return response({ userStatus: {} });
        const value = status(state.email, state.tier);
        if (state.legacy) value.userStatus.cascadeModelConfigData = { clientModelConfigs: state.legacy };
        return response(value);
      }
      assert.deepEqual(JSON.parse(init.body), { forceRefresh: true });
      if (state.afterQuota) state.afterQuota();
      if (state.quotaError) throw new Error('fake-secret transport error');
      return response(quota(state.left), state.quotaStatus, { 'retry-after': '600' });
    }
  };
  return { options, state, calls, processes, homeDir };
}

test('grouped quota preserves four distinct pools/windows and inverts remaining once', () => {
  const windows = parseQuota(quota());
  assert.deepEqual(windows.map(w => w.id), ['gemini-session', 'gemini-weekly', '3p-session', '3p-weekly']);
  assert.ok(Math.abs(windows[0].usedPercent - 20) < 1e-10);
  assert.equal(windows[1].usedPercent, 50);
  assert.equal(windows[2].usedPercent, 0);
  assert.equal(windows[0].resetsAt, Date.parse(RESET));
  assert.equal(windows[1].resetsAt, Date.parse(WEEK));
  assert.equal(headline(windows), 'gemini-session');
});

test('summary and quotaGroups envelopes, oneof, explicit zero and overage are real observations', () => {
  const groups = [{ displayName: 'Gemini Models', buckets: [
    { bucketId: 'weekly', remaining: { remainingFraction: 0 }, resetTime: NOW / 1000 },
    { bucketId: 'five-hour', used: '150', limit: '100' },
    { bucketId: 'disabled', disabled: true, remainingFraction: 0 },
    { bucketId: 'missing' }, { bucketId: 'invalid', remainingFraction: false }
  ] }];
  assert.equal(parseQuota({ summary: { groups } }).length, 2);
  assert.equal(parseQuota({ quotaGroups: groups }).length, 2);
  assert.equal(parseQuota({ groups })[0].usedPercent, 150);
  assert.equal(parseQuota({ groups })[1].usedPercent, 100);
  assert.equal(parseQuota({ groups })[1].resetsAt, NOW);
  assert.deepEqual(parseQuota({}), []);
});

test('expired resets do not invent a refill; model buckets never collapse into another model', () => {
  const windows = parseQuota({ buckets: [
    { modelId: 'gemini-pro', remainingFraction: 0.25, resetTime: '2020-01-01T00:00:00Z' },
    { modelId: 'gemini-flash', remainingFraction: 0.9, resetTime: 'invalid' }
  ] });
  assert.equal(windows.length, 2);
  assert.equal(windows[0].usedPercent, 75);
  assert.equal(windows[0].resetsAt, Date.UTC(2020, 0, 1));
  assert.equal(windows[1].resetsAt, null);
});

test('local actual tier takes precedence over planInfo and account identity is masked', () => {
  const parsed = parseStatus(status());
  assert.equal(parsed.account.plan, 'Google AI Ultra');
  assert.equal(parsed.account.label, 'o***@e***.com');
  assert.match(parsed.key, /^[a-f0-9]{64}$/);
  assert.ok(!JSON.stringify(parsed).includes('owner@example.com'));
});

test('process discovery rejects generic language servers, decoy arguments and malformed PID/token', () => {
  const entries = parseProcesses(PROCESS_TABLE, 'darwin');
  assert.equal(entries.length, 1);
  assert.deepEqual(entries[0], { pid: 42, csrfToken: CSRF, kind: 'ide' });
  assert.equal(parseProcesses(`99999999999999 ${COMMAND}`, 'darwin').length, 0);
  assert.equal(parseProcesses(`42 ${COMMAND.replace(CSRF, 'bad;header')}`, 'darwin').length, 0);
  assert.equal(parseProcesses('42 /Applications/Antigravity.app/bin/language_server_macos_arm', 'darwin').length, 0);
  assert.equal(parseProcesses('42 node /something/agy --csrf_token fixture', 'darwin').length, 0);
  assert.equal(parseProcesses('42 /opt/bin/agy --some-flag value', 'linux')[0].kind, 'cli');
});

test('socket discovery requires same PID, listening TCP and a local-bind address', () => {
  assert.deepEqual(parseLsof(LSOF + 'language_ 42 fixture 11u IPv4 2 0t0 TCP 192.0.2.5:7777 (LISTEN)\n', 42), [4317, 5555]);
  assert.deepEqual(parseNetstat('TCP 127.0.0.1:4444 0.0.0.0:0 LISTENING 42\n'
    + 'TCP [::]:5555 [::]:0 LISTENING 42\n'
    + 'TCP 127.0.0.1:6666 0.0.0.0:0 LISTENING 77\n', 42), [4444, 5555]);
});

test('JSON content alone cannot qualify an unrelated port to receive CSRF', () => {
  assert.equal(isRpcProof({ status: 200, contentType: 'application/json', body: { healthy: true } }, QUOTA_METHOD), false);
  assert.equal(isRpcProof({ status: 401, contentType: 'application/json', body: { error: 'unauthorized' } }, QUOTA_METHOD), false);
  assert.equal(isRpcProof({ status: 401, contentType: 'text/html', body: { code: 'unauthenticated', message: 'missing CSRF token' } }, QUOTA_METHOD), false);
  assert.equal(isRpcProof({ status: 401, contentType: 'application/json; charset=utf-8',
    body: { code: 'unauthenticated', message: 'missing CSRF token' } }, QUOTA_METHOD), true);
});

test('provider authenticates only discovered RPC, uses forceRefresh and reports local identity/plan', async t => {
  const f = await fixture(t);
  const record = await createProvider(f.options).read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(record.name, 'Antigravity');
  assert.equal(record.updatedAt, NOW);
  assert.equal(record.account.plan, 'Google AI Ultra');
  assert.equal(record.account.label, 'o***@e***.com');
  assert.equal(record.windows.length, 4);
  assert.ok(f.calls.filter(c => c.port === 4317).every(c => c.token === undefined));
  assert.ok(f.calls.some(c => c.port === 5555 && c.token === CSRF));
  const serialized = JSON.stringify(record);
  assert.ok(!serialized.includes(CSRF));
  assert.ok(!serialized.includes('owner@example.com'));
});

test('TTL cache still verifies current local account and invalidates on switch/sign-out', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  const quotaCalls = () => f.calls.filter(c => c.token && c.method === QUOTA_METHOD).length;
  const oldCount = quotaCalls();
  await provider.read({ now: NOW + 1 });
  assert.equal(quotaCalls(), oldCount);
  f.state.email = 'other@example.org';
  f.state.left = 0.1;
  const changed = await provider.read({ now: NOW + 2 });
  assert.equal(changed.account.label, 'o***@e***.org');
  assert.ok(Math.abs(changed.windows[0].usedPercent - 90) < 1e-10);
  assert.equal(quotaCalls(), oldCount + 1);
  f.state.signedOut = true;
  const signedOut = await provider.read({ now: NOW + 3 });
  assert.equal(signedOut.status, 'needs-auth');
  assert.deepEqual(signedOut.windows, []);
});

test('transient quota failure preserves original time only after same-account revalidation', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.state.quotaError = true;
  const stale = await provider.read({ now: NOW + TTL_MS + 1 });
  assert.equal(stale.status, 'stale');
  assert.equal(stale.updatedAt, NOW);
  assert.equal(stale.windows.length, 4);
  assert.ok(!stale.message.includes('fake-secret'));
});

test('account switch during successful quota read discards in-flight windows', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.state.afterQuota = () => { f.state.email = 'switched@example.org'; };
  const changed = await provider.read({ now: NOW + TTL_MS + 1 });
  assert.equal(changed.status, 'unavailable');
  assert.deepEqual(changed.windows, []);
});

test('final signed-out status with retained email discards quota and clears persistent cache', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.state.afterQuota = () => { f.state.signedOut = true; };
  const changed = await provider.read({ now: NOW + TTL_MS + 1 });
  assert.equal(changed.status, 'unavailable');
  assert.deepEqual(changed.windows, []);
  await assert.rejects(fs.readFile(path.join(f.options.storagePath, 'antigravity-quota.json')), { code: 'ENOENT' });
  assert.equal(parseStatus({ userStatus: { ...status().userStatus, isSignedIn: false } }), null);
});

test('sign-out during failed quota call cannot serve a previous account cache', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.state.quotaError = true;
  f.state.afterQuota = () => { f.state.signedOut = true; };
  const changed = await provider.read({ now: NOW + TTL_MS + 1 });
  assert.equal(changed.status, 'unavailable');
  assert.deepEqual(changed.windows, []);
});

test('legacy local GetUserStatus quota reports measured models without inventing weekly buckets', async t => {
  const f = await fixture(t);
  f.state.quotaStatus = 404;
  f.state.legacy = [
    { label: 'Gemini Pro', modelOrAlias: { model: 'gemini-pro' }, quotaInfo: { remainingFraction: 0.3, resetTime: RESET } },
    { label: 'Claude Sonnet', quotaInfo: { remainingFraction: 0.4, resetTime: RESET } }
  ];
  const record = await createProvider(f.options).read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(record.source, 'Antigravity local GetUserStatus');
  assert.equal(record.windows.length, 2);
  assert.match(record.message, /Weekly quota is unavailable/);
  assert.ok(!record.windows.some(w => /weekly/i.test(w.id)));
});

test('no process produces actionable IDE/server requirement without old observations', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.state.processes = '';
  const closed = await provider.read({ now: NOW + 1 });
  assert.equal(closed.status, 'unavailable');
  assert.match(closed.message, /local language server running/);
  assert.equal(closed.updatedAt, null);
  assert.deepEqual(closed.windows, []);
});

test('unidentified live quota is never retained as another account cached reading', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.state.unidentified = true;
  f.state.left = 0.3;
  const record = await provider.read({ now: NOW + 1 });
  assert.equal(record.status, 'ready');
  assert.equal(record.account.label, null);
  assert.match(record.message, /did not report an account identity/);
  assert.ok(Math.abs(record.windows[0].usedPercent - 70) < 1e-10);
  f.state.quotaError = true;
  const failed = await provider.read({ now: NOW + 2 });
  assert.equal(failed.status, 'unavailable');
  assert.deepEqual(failed.windows, []);
});

test('429 respects cooldown including forced calls; cache contains no email or CSRF', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  const cache = await fs.readFile(path.join(f.options.storagePath, 'antigravity-quota.json'), 'utf8');
  assert.ok(!cache.includes(CSRF));
  assert.ok(!cache.includes('owner@example.com'));
  f.state.quotaStatus = 429;
  const limited = await provider.read({ now: NOW + 1, force: true });
  assert.equal(limited.status, 'stale');
  const before = f.calls.length;
  const cooldown = await provider.read({ now: NOW + 2, force: true });
  assert.equal(cooldown.status, 'unavailable');
  assert.equal(f.calls.length, before);
});

test('Windows discovery uses current-SID process query and PID-scoped netstat sockets', async () => {
  const files = [];
  const command = '"C:\\Users\\Fixture\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server_windows_x64.exe" --csrf_token fixture-win-token';
  const endpoints = await discover({ platform: 'win32', execFile: async (file, args) => {
    files.push(file);
    if (file === 'powershell.exe') {
      assert.ok(args[args.indexOf('-Command') + 1].includes('GetOwnerSid'));
      return { stdout: JSON.stringify([{ ProcessId: 42, CommandLine: command,
        ExecutablePath: 'C:\\Users\\Fixture\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server_windows_x64.exe' }]) };
    }
    assert.equal(file, 'netstat.exe');
    return { stdout: 'TCP 127.0.0.1:5555 0.0.0.0:0 LISTENING 42\nTCP 127.0.0.1:6666 0.0.0.0:0 LISTENING 77\n' };
  } });
  assert.deepEqual(files, ['powershell.exe', 'netstat.exe']);
  assert.equal(endpoints.length, 1);
  assert.equal(endpoints[0].port, 5555);
  assert.equal(endpoints[0].csrfToken, 'fixture-win-token');
});

test('invalid endpoints and non-allowlisted methods are rejected before transport', async () => {
  let called = false;
  const options = { fetch: async () => { called = true; return response(quota()); } };
  await assert.rejects(rpcRequest({ pid: 42, port: 5555, scheme: 'https' }, '/arbitrary/path', {}, options),
    { code: 'invalid-endpoint' });
  await assert.rejects(rpcRequest({ pid: 42, port: 65536, scheme: 'https' }, QUOTA_METHOD, {}, options),
    { code: 'invalid-endpoint' });
  assert.equal(called, false);
});

test('missing process tools are reported without raw process or transport errors', async () => {
  const provider = createProvider({ platform: 'win32', execFile: async () => {
    const error = new Error('fixture-secret-payload'); error.code = 'ENOENT'; throw error;
  } });
  const record = await provider.read({ now: NOW });
  assert.equal(record.status, 'unavailable');
  assert.match(record.message, /PowerShell and netstat/);
  assert.ok(!JSON.stringify(record).includes('fixture-secret-payload'));
});

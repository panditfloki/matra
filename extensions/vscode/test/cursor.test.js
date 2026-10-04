'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { spawn, execFileSync } = require('node:child_process');
const { once } = require('node:events');
const { createProvider, parseUsage } = require('../src/providers/cursor');
const { credentials, claims, dataPaths, KEYS } = require('../src/providers/cursor-auth');
const { readItemTable } = require('../src/providers/cursor-sqlite');
const { runFile, fetchJson, MAX_BODY, TTL_MS } = require('../src/providers/cursor-runtime');

const NOW = Date.UTC(2026, 9, 5, 12);
const RESET = '2026-11-05T12:00:00.123Z';
const jwt = (sub = 'user_fixture', exp = NOW / 1000 + 86400) => `eyJhbGciOiJub25lIn0.${Buffer.from(JSON.stringify({ sub, exp })).toString('base64url')}.fixture`;
const summary = (auto = 12) => ({ membershipType: 'pro', billingCycleEnd: RESET,
  individualUsage: { plan: { autoPercentUsed: auto, apiPercentUsed: 27, totalPercentUsed: 19.5 } } });
const response = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), {
  status, headers: { 'content-type': 'application/json', ...headers }
});

async function fixture(t, extra = {}) {
  const homeDir = await fs.mkdtemp(path.join(__dirname, 'cursor-fixtures-'));
  t.after(() => fs.rm(homeDir, { recursive: true, force: true }));
  const dbPath = path.join(homeDir, 'state;$(must-not-execute).vscdb');
  await fs.writeFile(dbPath, 'test-only placeholder');
  let rows = { 'cursorAuth/accessToken': jwt(), 'cursorAuth/stripeMembershipAuthId': 'user_fixture',
    'cursorAuth/cachedEmail': 'owner@example.com', 'cursorAuth/stripeMembershipType': 'pro' };
  const calls = [];
  const options = { homeDir, platform: 'linux', paths: { stateDb: dbPath },
    execFile: async (file, args, settings) => {
      calls.push({ file, args, settings });
      assert.equal(settings.shell, false);
      assert.equal(settings.maxBuffer, MAX_BODY);
      return { stdout: JSON.stringify(Object.entries(rows).map(([key, value]) => ({ key, value }))) };
    }, fetch: async () => response(summary()), ...extra };
  return { options, calls, homeDir, dbPath, setRows: value => { rows = value; }, getRows: () => rows };
}

test('Cursor keeps explicit Auto, API, on-demand and team windows separate', () => {
  const parsed = parseUsage({ ...summary(0), individualUsage: {
    plan: { used: 0, limit: 0, autoPercentUsed: 0, apiPercentUsed: 19, totalPercentUsed: 9.5 },
    onDemand: { enabled: true, used: 15, limit: 10 }
  }, teamUsage: { onDemand: { enabled: true, used: 0, limit: 100 } } });
  assert.equal(parsed.headlineId, 'auto');
  assert.deepEqual(parsed.windows.map(w => [w.id, w.usedPercent]),
    [['auto', 0], ['api', 19], ['on_demand', 150], ['team_on_demand', 0]]);
  assert.ok(parsed.windows.every(w => w.resetsAt === Date.parse(RESET)));
  assert.ok(!parsed.windows.some(w => w.usedPercent === 9.5));
});

test('enterprise headline uses overall allowance even with on-demand spend', () => {
  const parsed = parseUsage({ membershipType: 'enterprise', individualUsage: {
    overall: { enabled: true, used: 6907, limit: 45000 },
    onDemand: { enabled: true, used: 10, limit: 100 }
  } });
  assert.equal(parsed.headlineId, 'included');
  assert.equal(parsed.windows[0].usedPercent, 6907 / 45000 * 100);
  assert.equal(parsed.windows[0].resetsAt, null);
});

test('zero API is measured; absent, disabled and unlimited quota never becomes zero', () => {
  assert.equal(parseUsage({ individualUsage: { plan: { apiPercentUsed: 0 } } }).windows[0].usedPercent, 0);
  assert.deepEqual(parseUsage({ isUnlimited: true, individualUsage: {
    plan: { used: 0, limit: 0 }, onDemand: { enabled: false, used: 0, limit: 10 }
  } }).windows, []);
  assert.deepEqual(parseUsage({ individualUsage: { plan: { autoPercentUsed: false, apiPercentUsed: Infinity } } }).windows, []);
  assert.equal(parseUsage('malformed'), null);
});

test('expired reset preserves the observed percentage and unknown reset remains null', () => {
  const parsed = parseUsage({ ...summary(122.25), billingCycleEnd: '2020-01-01T00:00:00Z' });
  assert.equal(parsed.windows[0].usedPercent, 122.25);
  assert.equal(parsed.windows[0].resetsAt, Date.UTC(2020, 0, 1));
  assert.equal(parseUsage({ ...summary(), billingCycleEnd: 'not a date' }).windows[0].resetsAt, null);
});

test('cookie requires accountID::token, with JWT sub fallback and masked email', () => {
  const auth = credentials(jwt('auth0|fixture'), '', 'owner@example.com', 'pro', 'Cursor editor', NOW);
  assert.equal(auth.cookie, `WorkosCursorSessionToken=auth0|fixture::${jwt('auth0|fixture')}`);
  assert.equal(auth.account.label, 'o***@e***.com');
  assert.equal(claims('bad token'), null);
  assert.throws(() => credentials(jwt(), 'bad;injected', null, null, 'Cursor', NOW));
  assert.throws(() => credentials('bad\r\nHeader', 'user_fixture', null, null, 'Cursor', NOW));
  assert.throws(() => credentials(jwt('user_fixture', NOW / 1000), null, null, null, 'Cursor', NOW),
    { code: 'expired-auth' });
  assert.notEqual(credentials(jwt('AccountA'), 'AccountA', null, null, 'Cursor', NOW).key,
    credentials(jwt('accounta'), 'accounta', null, null, 'Cursor', NOW).key);
});

test('provider queries only fixed Cursor endpoint, preserves source plan and redacts all credentials', async t => {
  const f = await fixture(t);
  f.options.fetch = async (url, init) => {
    assert.equal(url, 'https://cursor.com/api/usage-summary');
    assert.equal(init.headers.Cookie, `WorkosCursorSessionToken=user_fixture::${jwt()}`);
    assert.equal(init.redirect, 'error');
    assert.ok(init.signal);
    return response(summary());
  };
  const record = await createProvider(f.options).read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(record.updatedAt, NOW);
  assert.equal(record.account.plan, 'pro');
  assert.ok(!JSON.stringify(record).includes(jwt()));
  assert.ok(!JSON.stringify(record).includes('owner@example.com'));
  assert.equal(f.calls[0].args[0], '-readonly');
  assert.ok(f.calls[0].args.includes(f.dbPath));
  assert.ok(!f.calls[0].args.join(' ').includes('immutable'));
});

test('same-account transient service failure preserves observation time as stale', async t => {
  const f = await fixture(t);
  const provider = createProvider(f.options);
  const ready = await provider.read({ now: NOW });
  f.options.fetch = undefined;
  // The provider captures the injected transport, so switch its implementation through a closure.
  let fail = false;
  const again = createProvider({ ...f.options, fetch: async () => {
    if (fail) throw new Error('secret fixture stderr must not escape');
    return response(summary());
  } });
  await again.read({ now: NOW }); fail = true;
  const stale = await again.read({ now: NOW + TTL_MS + 1 });
  assert.equal(stale.status, 'stale');
  assert.equal(stale.updatedAt, ready.updatedAt);
  assert.equal(stale.windows[0].usedPercent, 12);
  assert.ok(!stale.message.includes('secret fixture'));
});

test('sign-out invalidates a fresh cache and never fetches without credentials', async t => {
  let requests = 0;
  const f = await fixture(t, { fetch: async () => { requests++; return response(summary()); } });
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.setRows({});
  const signedOut = await provider.read({ now: NOW + 1 });
  assert.equal(signedOut.status, 'needs-auth');
  assert.deepEqual(signedOut.windows, []);
  assert.equal(signedOut.updatedAt, null);
  assert.equal(requests, 1);
});

test('account change never retains another account quota after network failure', async t => {
  let fail = false;
  const f = await fixture(t, { fetch: async () => {
    if (fail) throw new Error('network fixture');
    return response(summary(85));
  } });
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  f.setRows({ ...f.getRows(), 'cursorAuth/stripeMembershipAuthId': 'user_new',
    'cursorAuth/accessToken': jwt('user_new'), 'cursorAuth/cachedEmail': 'new@example.org' });
  fail = true;
  const changed = await provider.read({ now: NOW + 1 });
  assert.equal(changed.status, 'unavailable');
  assert.deepEqual(changed.windows, []);
  assert.equal(changed.account.label, 'n***@e***.org');
});

test('sign-out or account change during a failed request cannot serve stale quota', async t => {
  let action;
  const f = await fixture(t, { fetch: async () => {
    if (action) { action(); throw new Error('offline'); }
    return response(summary(85));
  } });
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  action = () => f.setRows({});
  const record = await provider.read({ now: NOW + TTL_MS + 1 });
  assert.equal(record.status, 'needs-auth');
  assert.deepEqual(record.windows, []);
});

test('account switch during successful request discards the in-flight quota', async t => {
  const f = await fixture(t);
  f.options.fetch = async () => {
    f.setRows({ ...f.getRows(), 'cursorAuth/stripeMembershipAuthId': 'user_new', 'cursorAuth/accessToken': jwt('user_new') });
    return response(summary(85));
  };
  const record = await createProvider(f.options).read({ now: NOW });
  assert.equal(record.status, 'unavailable');
  assert.deepEqual(record.windows, []);
});

test('401 clears cached quota; 429 honors Retry-After even on forced refresh', async t => {
  let status = 200;
  let requests = 0;
  const f = await fixture(t, { fetch: async () => { requests++; return response(summary(), status, { 'retry-after': '600' }); } });
  const provider = createProvider(f.options);
  await provider.read({ now: NOW });
  status = 429;
  const limited = await provider.read({ now: NOW + 1, force: true });
  assert.equal(limited.status, 'stale');
  const before = requests;
  assert.equal((await provider.read({ now: NOW + 2, force: true })).status, 'stale');
  assert.equal((await provider.read({ now: NOW + 3 })).status, 'stale');
  assert.equal(requests, before);
  status = 401;
  const rejected = await provider.read({ now: NOW + 601000, force: true });
  assert.equal(rejected.status, 'needs-auth');
  assert.deepEqual(rejected.windows, []);
});

test('cache persists only masked observations and a validated account hash', async t => {
  const f = await fixture(t);
  f.options.storagePath = path.join(f.homeDir, 'extension-cache');
  await createProvider(f.options).read({ now: NOW });
  const file = await fs.readFile(path.join(f.options.storagePath, 'cursor-quota.json'), 'utf8');
  assert.ok(!file.includes(jwt()));
  assert.ok(!file.includes('owner@example.com'));
  assert.ok(!file.includes('user_fixture'));
  const restored = await createProvider({ ...f.options, fetch: async () => { throw new Error('not needed'); } }).read({ now: NOW + 1 });
  assert.equal(restored.status, 'ready');
  f.setRows({ ...f.getRows(), 'cursorAuth/stripeMembershipAuthId': 'user_other' });
  const other = await createProvider({ ...f.options, fetch: async () => { throw new Error('offline'); } }).read({ now: NOW + 1 });
  assert.deepEqual(other.windows, []);
});

test('missing SQLite tools produce an actionable unmet dependency state', async t => {
  const f = await fixture(t, { execFile: async () => { const error = new Error('not installed'); error.code = 'ENOENT'; throw error; } });
  const record = await createProvider(f.options).read({ now: NOW });
  assert.equal(record.status, 'unavailable');
  assert.match(record.message, /SQLite.*sqlite3.*Python 3/);
});

test('SQLite fallback uses Windows py -3 stdlib with read-only URI and bound keys', async t => {
  const f = await fixture(t);
  const files = [];
  const rows = await readItemTable(f.dbPath, KEYS, { platform: 'win32', execFile: async (file, args) => {
    files.push(file);
    if (file === 'sqlite3') { const error = new Error('absent'); error.code = 'ENOENT'; throw error; }
    assert.equal(file, 'py');
    assert.deepEqual(args.slice(0, 2), ['-3', '-c']);
    assert.ok(args[2].includes('?mode=ro'));
    assert.ok(!args[2].includes('immutable'));
    assert.equal(args[3], f.dbPath);
    assert.deepEqual(JSON.parse(args[4]), KEYS);
    return { stdout: JSON.stringify([{ key: KEYS[0], value: 'fixture-only' }]) };
  } });
  assert.equal(rows[KEYS[0]], 'fixture-only');
  assert.deepEqual(files, ['sqlite3', 'py']);
});

test('macOS cursor-agent keychain fallback uses authId before JWT sub', async t => {
  const f = await fixture(t);
  f.setRows({});
  const config = path.join(f.homeDir, '.cursor', 'cli-config.json');
  await fs.mkdir(path.dirname(config));
  await fs.writeFile(config, JSON.stringify({ authInfo: { email: 'cli@example.org', authId: 'user_cli' } }));
  f.options.platform = 'darwin';
  const dbReader = f.options.execFile;
  f.options.execFile = async (file, args, settings) => file === '/usr/bin/security'
    ? { stdout: jwt('subject_fallback') } : dbReader(file, args, settings);
  f.options.fetch = async (url, init) => {
    assert.equal(init.headers.Cookie, `WorkosCursorSessionToken=user_cli::${jwt('subject_fallback')}`);
    return response(summary());
  };
  const record = await createProvider(f.options).read({ now: NOW });
  assert.equal(record.status, 'ready');
  assert.equal(record.source, 'cursor-agent');
  assert.equal(record.account.label, 'c***@e***.org');
});

test('platform defaults use provider app data directories and explicit override wins', () => {
  assert.match(dataPaths({ homeDir: '/fixture', platform: 'darwin' }).stateDb,
    /Library\/Application Support\/Cursor\/User\/globalStorage\/state.vscdb$/);
  assert.match(dataPaths({ homeDir: '/fixture', platform: 'win32' }).stateDb,
    /(?:AppData\/Roaming|Roaming).*Cursor\/User\/globalStorage\/state.vscdb$/);
  assert.match(dataPaths({ homeDir: '/fixture', platform: 'linux' }).stateDb,
    /Cursor\/User\/globalStorage\/state.vscdb$/);
  assert.equal(dataPaths({ paths: { stateDb: '/explicit/fixture.db' } }).stateDb, '/explicit/fixture.db');
});

test('runtime caps process output, strips raw errors and bounds a hung transport', async () => {
  await assert.rejects(runFile(async () => ({ stdout: 'x'.repeat(MAX_BODY + 1) }), 'fixture', []), { code: 'too-large' });
  await assert.rejects(runFile(async () => { throw new Error('fake-secret'); }, 'fixture', []),
    error => error.message === 'command-failed');
  await assert.rejects(fetchJson(async () => new Promise(() => {}), 'https://fixture.invalid', {}, 10), { code: 'network-failed' });
  await assert.rejects(fetchJson(async () => response({}, 200, { 'content-length': String(MAX_BODY + 1) }),
    'https://fixture.invalid', {}), { code: 'too-large' });
});

test('real SQLite reader sees an uncheckpointed WAL token without changing DB or WAL', async t => {
  try { execFileSync('python3', ['-c', 'import sqlite3'], { timeout: 5000, stdio: 'ignore' }); }
  catch { t.skip('Python 3 stdlib unavailable for WAL fixture generation'); return; }
  const f = await fixture(t);
  await fs.unlink(f.dbPath);
  const script = [
    'import sqlite3,sys',
    'db=sqlite3.connect(sys.argv[1])',
    'db.execute("PRAGMA journal_mode=WAL")',
    'db.execute("PRAGMA wal_autocheckpoint=0")',
    'db.execute("CREATE TABLE ItemTable (key TEXT PRIMARY KEY,value TEXT)")',
    'db.commit()',
    'db.execute("PRAGMA wal_checkpoint(TRUNCATE)")',
    'db.execute("INSERT INTO ItemTable VALUES (?,?)",("cursorAuth/accessToken","fixture-wal-rotated"))',
    'db.commit()',
    'print("ready",flush=True)',
    'sys.stdin.readline()',
    'db.close()'
  ].join('\n');
  const writer = spawn('python3', ['-c', script, f.dbPath], { stdio: ['pipe', 'pipe', 'pipe'] });
  const exited = once(writer, 'exit');
  t.after(async () => { writer.stdin.end('\n'); await exited; });
  await once(writer.stdout, 'data');
  const beforeDb = await fs.readFile(f.dbPath);
  const beforeWal = await fs.readFile(`${f.dbPath}-wal`);
  const rows = await readItemTable(f.dbPath, KEYS);
  assert.equal(rows[KEYS[0]], 'fixture-wal-rotated');
  assert.deepEqual(await fs.readFile(f.dbPath), beforeDb);
  assert.deepEqual(await fs.readFile(`${f.dbPath}-wal`), beforeWal);
});

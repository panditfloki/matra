'use strict';

const os = require('node:os');
const path = require('node:path');
const { readItemTable } = require('./cursor-sqlite');
const { exists, readText, runFile, failure, accountKey, maskIdentity, safePlan } = require('./cursor-runtime');

const KEYS = [
  'cursorAuth/accessToken', 'cursorAuth/stripeMembershipAuthId',
  'cursorAuth/cachedEmail', 'cursorAuth/stripeMembershipType'
];

function dataPaths(options = {}) {
  const home = options.homeDir || os.homedir();
  const platform = options.platform || process.platform;
  const appData = platform === 'darwin' ? path.join(home, 'Library', 'Application Support')
    : platform === 'win32' ? process.env.APPDATA || path.join(home, 'AppData', 'Roaming')
      : process.env.XDG_CONFIG_HOME || path.join(home, '.config');
  return {
    stateDb: options.paths?.stateDb || options.paths?.cursorStateDb
      || path.join(appData, 'Cursor', 'User', 'globalStorage', 'state.vscdb'),
    // Some Linux distributions use a lower-case app directory.
    alternateDb: platform === 'linux' && !options.paths?.stateDb && !options.paths?.cursorStateDb
      ? path.join(appData, 'cursor', 'User', 'globalStorage', 'state.vscdb') : null,
    cliConfig: options.paths?.cliConfig || path.join(home, '.cursor', 'cli-config.json')
  };
}

function claims(token) {
  if (typeof token !== 'string' || token.length > 16384) return null;
  const pieces = token.split('.');
  if (pieces.length !== 3 || !/^[A-Za-z0-9_-]+$/.test(pieces[1])) return null;
  try {
    const value = JSON.parse(Buffer.from(pieces[1], 'base64url').toString('utf8'));
    return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
  } catch { return null; }
}

function credentials(token, id, email, plan, source, now) {
  const accessToken = typeof token === 'string' ? token.trim() : '';
  const payload = claims(accessToken);
  const accountID = typeof id === 'string' && id.trim() ? id.trim() : payload?.sub;
  if (!/^[A-Za-z0-9._~+/=-]{1,16384}$/.test(accessToken)
      || typeof accountID !== 'string' || !/^[A-Za-z0-9_|:@.-]{1,512}$/.test(accountID)
      || accountID.includes('::')) throw failure('needs-auth');
  if (typeof payload?.exp === 'number' && Number.isFinite(payload.exp) && payload.exp * 1000 <= now) {
    throw failure('expired-auth');
  }
  return {
    cookie: `WorkosCursorSessionToken=${accountID}::${accessToken}`,
    key: accountKey(accountID),
    account: { label: maskIdentity(email), plan: safePlan(plan) },
    source
  };
}

async function loadCredentials(options, now) {
  const paths = dataPaths(options);
  const primaryExists = await exists(paths.stateDb);
  const dbPath = primaryExists ? paths.stateDb
    : paths.alternateDb && await exists(paths.alternateDb) ? paths.alternateDb : null;
  if (dbPath) {
    // A dependency or unreadable DB is not a sign-out. Do not silently choose another account.
    const rows = await readItemTable(dbPath, KEYS, options);
    if (rows['cursorAuth/accessToken']?.trim()) {
      return credentials(rows['cursorAuth/accessToken'], rows['cursorAuth/stripeMembershipAuthId'],
        rows['cursorAuth/cachedEmail'], rows['cursorAuth/stripeMembershipType'], 'Cursor editor', now);
    }
  }

  // The native source establishes this macOS CLI keychain item. Other OS keyrings
  // have no verified portable reader, so those users are directed to the editor.
  if ((options.platform || process.platform) === 'darwin' && await exists(paths.cliConfig)) {
    let authInfo;
    try { authInfo = JSON.parse(await readText(paths.cliConfig)).authInfo; } catch { }
    if (authInfo && typeof authInfo === 'object') {
      let token;
      try {
        token = await runFile(options.execFile, '/usr/bin/security', [
          'find-generic-password', '-s', 'cursor-access-token', '-a', 'cursor-user', '-w'
        ]);
      } catch (error) {
        throw failure(error.code === 'timeout' ? 'keychain-unavailable' : 'needs-auth');
      }
      const id = authInfo.authId || (typeof authInfo.userId === 'number' || typeof authInfo.userId === 'string'
        ? String(authInfo.userId) : null);
      return credentials(token, id, authInfo.email, null, 'cursor-agent', now);
    }
  }
  throw failure('needs-auth');
}

module.exports = { dataPaths, claims, credentials, loadCredentials, KEYS };

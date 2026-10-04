'use strict';

const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFile: systemExecFile } = require('node:child_process');

const MAX_BODY = 1024 * 1024;
const MAX_FILE = 256 * 1024;
const TIMEOUT_MS = 5000;
const TTL_MS = 5 * 60 * 1000;

function failure(code) {
  const error = new Error(code);
  error.code = code;
  return error;
}

// Errors and stderr can contain credentials. Only fixed error codes escape this module.
function runFile(execFile, file, args, timeout = TIMEOUT_MS) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let child;
    const timer = setTimeout(() => {
      child?.kill?.();
      finish(failure('timeout'));
    }, timeout);
    function finish(error, stdout = '') {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) {
        const code = error.code === 'ENOENT' ? 'missing-command'
          : error.code === 'timeout' || error.killed ? 'timeout' : 'command-failed';
        return reject(failure(code));
      }
      const text = Buffer.isBuffer(stdout) ? stdout.toString('utf8') : String(stdout ?? '');
      if (Buffer.byteLength(text) > MAX_BODY) return reject(failure('too-large'));
      resolve(text);
    }
    try {
      child = (execFile || systemExecFile)(file, args, {
        encoding: 'utf8', timeout, maxBuffer: MAX_BODY, windowsHide: true, shell: false
      }, (error, stdout) => finish(error, stdout));
      if (child?.then) child.then(
        result => finish(null, typeof result === 'string' ? result : result?.stdout),
        error => finish(error)
      );
    } catch (error) { finish(error); }
  });
}

async function readText(file, limit = MAX_FILE) {
  const handle = await fs.open(file, 'r');
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.size > limit) throw failure('too-large');
    const buffer = Buffer.alloc(limit + 1);
    let size = 0;
    while (size < buffer.length) {
      const { bytesRead } = await handle.read(buffer, size, buffer.length - size, null);
      if (!bytesRead) break;
      size += bytesRead;
    }
    if (size > limit) throw failure('too-large');
    return buffer.subarray(0, size).toString('utf8');
  } finally { await handle.close(); }
}

async function exists(file) {
  try { return (await fs.stat(file)).isFile(); } catch { return false; }
}

function accountKey(value) {
  return crypto.createHash('sha256').update(String(value).trim()).digest('hex');
}

function maskIdentity(value) {
  if (typeof value !== 'string') return null;
  const match = /^([^\s@]+)@([^\s@]+)$/.exec(value.trim());
  if (!match) return null;
  const suffix = /\.[a-z]{2,12}$/i.exec(match[2])?.[0] || '';
  return `${match[1][0]}***@${match[2][0]}***${suffix}`;
}

function safePlan(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed && trimmed.length <= 80 && /^[\p{L}\p{N} ()+._-]+$/u.test(trimmed)
    ? trimmed : null;
}

function epoch(value) {
  if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
    return value < 1e11 ? value * 1000 : value;
  }
  if (typeof value !== 'string' || !value.trim()) return null;
  if (/^\d+(?:\.\d+)?$/.test(value)) return epoch(Number(value));
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function finiteNumber(value) {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null;
  if (typeof value === 'string' && /^-?\d+(?:\.\d+)?$/.test(value.trim())) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

async function responseBody(response, signal) {
  if (Number(response.headers?.get?.('content-length')) > MAX_BODY) throw failure('too-large');
  let size = 0;
  const parts = [];
  if (response.body && typeof response.body[Symbol.asyncIterator] === 'function') {
    for await (const part of response.body) {
      if (signal.aborted) throw failure('timeout');
      const bytes = Buffer.from(part);
      size += bytes.length;
      if (size > MAX_BODY) {
        try { await response.body.cancel?.(); } catch { }
        throw failure('too-large');
      }
      parts.push(bytes);
    }
    return Buffer.concat(parts, size).toString('utf8');
  }
  const text = await response.text();
  if (Buffer.byteLength(text) > MAX_BODY) throw failure('too-large');
  return text;
}

async function fetchJson(fetchImpl, url, init, timeout = TIMEOUT_MS) {
  const controller = new AbortController();
  let timer;
  const operation = (async () => {
    const response = await fetchImpl(url, { ...init, signal: controller.signal, redirect: 'error' });
    const text = await responseBody(response, controller.signal);
    let body = null;
    try { body = JSON.parse(text); } catch { }
    return {
      status: Number(response.status),
      contentType: response.headers?.get?.('content-type') || '',
      retryAfter: response.headers?.get?.('retry-after') || null,
      body
    };
  })();
  const deadline = new Promise((resolve, reject) => {
    timer = setTimeout(() => { controller.abort(); reject(failure('timeout')); }, timeout);
  });
  try { return await Promise.race([operation, deadline]); }
  catch (error) { throw failure(error.code === 'too-large' ? 'too-large' : 'network-failed'); }
  finally { clearTimeout(timer); controller.abort(); }
}

function retryAt(header, now) {
  const seconds = finiteNumber(header);
  const at = seconds !== null ? now + seconds * 1000 : epoch(header);
  return Math.min(now + 60 * 60 * 1000, Math.max(now + 60 * 1000, at || now + TTL_MS));
}

function copy(value) { return JSON.parse(JSON.stringify(value)); }

// Only extension-owned observations are saved. An identity hash must be verified anew
// before loading a record; a missing account never qualifies for stale replay.
function createCache(storagePath, id) {
  const file = typeof storagePath === 'string' && path.isAbsolute(storagePath)
    ? path.join(storagePath, `${id}-quota.json`) : null;
  let held = null;
  let loaded = false;
  return {
    async get(key) {
      if (!loaded) {
        loaded = true;
        if (file) {
          try {
            const value = JSON.parse(await readText(file));
            const record = value?.record;
            if (value.schemaVersion === 1 && /^[a-f0-9]{64}$/.test(value.accountKey)
                && record?.id === id && Number.isFinite(record.updatedAt)
                && Array.isArray(record.windows) && record.windows.length <= 64
                && record.windows.every(w => typeof w.id === 'string' && w.id.length <= 160
                  && typeof w.label === 'string' && w.label.length <= 160
                  && (w.usedPercent === null || Number.isFinite(w.usedPercent))
                  && (w.resetsAt === null || Number.isFinite(w.resetsAt)))) {
              held = value;
            }
          } catch { }
        }
      }
      return held?.accountKey === key ? copy(held.record) : null;
    },
    async put(key, record) {
      loaded = true;
      held = { schemaVersion: 1, accountKey: key, record: copy(record) };
      if (file) {
        const temporary = `${file}.${process.pid}.${crypto.randomBytes(4).toString('hex')}.tmp`;
        try {
          await fs.mkdir(storagePath, { recursive: true, mode: 0o700 });
          await fs.writeFile(temporary, JSON.stringify(held), { mode: 0o600 });
          await fs.rename(temporary, file);
        } catch { try { await fs.unlink(temporary); } catch { } }
      }
    },
    async clear() {
      loaded = true;
      held = null;
      if (file) { try { await fs.unlink(file); } catch { } }
    }
  };
}

module.exports = {
  MAX_BODY, MAX_FILE, TIMEOUT_MS, TTL_MS, failure, runFile, readText, exists,
  accountKey, maskIdentity, safePlan, epoch, finiteNumber, fetchJson, retryAt, createCache
};

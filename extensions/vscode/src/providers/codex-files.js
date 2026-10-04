'use strict';

// Bounded, read-only local metadata I/O shared by the Claude and Codex readers.
const fs = require('node:fs');
const fsp = fs.promises;
const path = require('node:path');
const crypto = require('node:crypto');

const MAX_FILES = 500;
const TAIL_BYTES = 1024 * 1024;
const MAX_SCAN_BYTES = 32 * 1024 * 1024;
const yieldHost = () => new Promise(resolve => setImmediate(resolve));
const number = value => typeof value === 'number' && Number.isFinite(value) ? value : null;
const count = value => number(value) !== null && value >= 0 && Number.isSafeInteger(value) ? value : null;
const clean = (value, limit = 80) => typeof value === 'string'
  ? value.replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, limit) : '';
const opaque = value => crypto.createHash('sha256').update(String(value)).digest('hex');

function epoch(value) {
  if (typeof value === 'string' && !/^\d+(\.\d+)?$/.test(value)) {
    const parsed = Date.parse(value);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }
  const numeric = typeof value === 'string' && value.trim() ? Number(value) : value;
  if (number(numeric) === null || numeric <= 0) return null;
  const parsed = numeric < 100_000_000_000 ? numeric * 1000 : numeric;
  return parsed <= 8_640_000_000_000_000 ? parsed : null;
}

function absoluteRoot(value, fallback) {
  const selected = value === undefined ? fallback : value;
  if (typeof selected !== 'string' || selected.includes('\0') || !path.isAbsolute(selected)) return null;
  const result = path.normalize(selected);
  return result === path.parse(result).root ? null : result;
}

function projectLabel(cwd) {
  if (typeof cwd !== 'string') return 'Local project';
  return clean(cwd.replace(/\\/g, '/').replace(/\/+$/, '').split('/').pop()) || 'Local project';
}

function maskedEmail(value) {
  const email = clean(value, 200);
  const match = /^([^@\s]+)@([a-z0-9.-]+\.[a-z]{2,})$/i.exec(email);
  return match ? `${match[1].slice(0, 1)}***@${match[2]}` : null;
}

async function readJson(file, maxBytes = 128 * 1024) {
  let handle;
  try {
    const stat = await fsp.lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > maxBytes) return { kind: 'invalid', mtime: stat.mtimeMs };
    handle = await fsp.open(file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
    const data = Buffer.alloc(maxBytes + 1);
    const { bytesRead } = await handle.read(data, 0, data.length, 0);
    if (bytesRead > maxBytes) return { kind: 'invalid', mtime: stat.mtimeMs };
    const value = JSON.parse(data.subarray(0, bytesRead).toString('utf8'));
    if (!value || typeof value !== 'object' || Array.isArray(value)) return { kind: 'invalid', mtime: stat.mtimeMs };
    return { kind: 'ok', value, mtime: stat.mtimeMs };
  } catch (error) {
    return { kind: error.code === 'ENOENT' ? 'missing' : error instanceof SyntaxError ? 'invalid' : 'error' };
  } finally {
    if (handle) await handle.close().catch(() => {});
  }
}

async function recentFiles(roots, budget) {
  const queue = roots.map(root => ({ root, depth: 0 }));
  const files = [];
  let entries = 0;
  let directories = 0;
  let readableRoots = 0;
  while (queue.length && directories < 2000 && entries < 20_000 && Date.now() < budget.deadline) {
    const { root, depth } = queue.shift();
    directories++;
    try {
      if ((await fsp.lstat(root)).isSymbolicLink()) { budget.partial = true; continue; }
      const dir = await fsp.opendir(root);
      if (depth === 0) readableRoots++;
      for await (const entry of dir) {
        if (++entries > 20_000 || Date.now() >= budget.deadline) { budget.partial = true; break; }
        const file = path.join(root, entry.name);
        if (entry.isDirectory() && depth < 7) queue.push({ root: file, depth: depth + 1 });
        else if (entry.isFile() && entry.name.endsWith('.jsonl')) {
          try {
            const stat = await fsp.lstat(file);
            if (stat.isFile()) files.push({ file, mtime: stat.mtimeMs, size: stat.size });
          } catch { budget.partial = true; }
        }
        if (entries % 100 === 0) await yieldHost();
      }
    } catch (error) {
      if (error.code !== 'ENOENT') budget.partial = true;
    }
  }
  if (queue.length || files.length > MAX_FILES) budget.partial = true;
  files.sort((a, b) => b.mtime - a.mtime || a.file.localeCompare(b.file));
  return { files: files.slice(0, MAX_FILES), readableRoots };
}

async function readJsonl(item, budget, onRow) {
  let handle;
  if (budget.bytes >= MAX_SCAN_BYTES || Date.now() >= budget.deadline) { budget.partial = true; return; }
  try {
    handle = await fsp.open(item.file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
    const stat = await handle.stat();
    if (!stat.isFile()) { budget.partial = true; return; }
    const length = Math.min(stat.size, TAIL_BYTES, MAX_SCAN_BYTES - budget.bytes);
    const start = stat.size - length;
    const buf = Buffer.alloc(length);
    const { bytesRead } = await handle.read(buf, 0, length, start);
    budget.bytes += bytesRead;
    if (start > 0) budget.partial = true;
    // The header is read separately when it has fallen outside the tail.
    if (start > 0) {
      const head = Buffer.alloc(Math.min(64 * 1024, start));
      const read = await handle.read(head, 0, head.length, 0);
      const line = head.subarray(0, read.bytesRead).toString('utf8').split('\n')[0];
      parseRow(line, onRow, { header: true, full: false, index: -1 });
    }
    let content = buf.subarray(0, bytesRead).toString('utf8');
    if (start > 0) content = content.slice(content.indexOf('\n') + 1);
    const lines = content.split('\n');
    for (let index = 0; index < lines.length; index++) {
      if (Date.now() >= budget.deadline) { budget.partial = true; break; }
      parseRow(lines[index], onRow, { header: false, full: start === 0, index });
      if (index % 100 === 0) await yieldHost();
    }
  } catch { budget.partial = true; }
  finally { if (handle) await handle.close().catch(() => {}); }
}

function parseRow(line, onRow, meta) {
  // Ignore message/prompt events before parsing. Consumers only inspect usage,
  // session headers and turn metadata, and never retain transcript content.
  if (!/"type"\s*:\s*"(?:assistant|session_meta|event_msg|turn_context)"/.test(line)) return;
  try { onRow(JSON.parse(line), meta); } catch { /* incomplete append or malformed row */ }
}

function scanBudget() { return { bytes: 0, deadline: Date.now() + 8000, partial: false }; }
function startOfDay(now) { const date = new Date(now); date.setHours(0, 0, 0, 0); return date.getTime(); }
function within(file, root) { const relative = path.relative(root, file); return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative)); }

function cacheFile(storagePath, root, name) {
  const storage = absoluteRoot(storagePath);
  return storage && !within(storage, root) ? path.join(storage, name) : null;
}

async function writeCache(file, data) {
  if (!file) return;
  const temporary = `${file}.${process.pid}.${crypto.randomBytes(4).toString('hex')}.tmp`;
  try {
    await fsp.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
    // Extension-owned cache only; no credential or provider file is written.
    await fsp.writeFile(temporary, JSON.stringify(data), { mode: 0o600, flag: 'wx' });
    await fsp.rename(temporary, file);
  } catch { await fsp.unlink(temporary).catch(() => {}); }
}

async function clearCache(file) { if (file) await fsp.unlink(file).catch(() => {}); }

module.exports = { number, count, clean, opaque, epoch, absoluteRoot, projectLabel, maskedEmail,
  readJson, recentFiles, readJsonl, scanBudget, startOfDay, cacheFile, writeCache, clearCache };

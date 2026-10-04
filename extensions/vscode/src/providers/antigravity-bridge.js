'use strict';

const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const https = require('node:https');
const { runFile, readText, failure, MAX_BODY, fetchJson } = require('./cursor-runtime');
const { parseQuota, parseStatus } = require('./antigravity-quota');

const QUOTA_METHOD = '/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary';
const STATUS_METHOD = '/exa.language_server_pb.LanguageServerService/GetUserStatus';
const RPC_TIMEOUT_MS = 4000;
const DISCOVERY_MS = 16000;

// Static PowerShell, no user data is interpolated into executable code. Limit results
// and require the current Windows SID so another user's process is never borrowed.
const WINDOWS_PROCESSES = [
  '$ErrorActionPreference="Stop";',
  '$sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value;',
  '@(Get-CimInstance Win32_Process | Where-Object {$_.Name -match "^(language[_-]server.*|agy|antigravity[_-]cli)\\.exe$"} | Select-Object -First 64 | ForEach-Object {',
  '$owner=Invoke-CimMethod -InputObject $_ -MethodName GetOwnerSid;',
  'if($owner.Sid -eq $sid){[PSCustomObject]@{ProcessId=$_.ProcessId;CommandLine=$_.CommandLine;ExecutablePath=$_.ExecutablePath}}',
  '}) | ConvertTo-Json -Compress'
].join(' ');

function validPid(pid) { return Number.isSafeInteger(pid) && pid > 0 && pid <= 0x7fffffff; }
function validPort(port) { return Number.isSafeInteger(port) && port > 0 && port <= 65535; }

function flag(command, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = new RegExp(`(?:^|\\s)${escaped}(?:=|\\s+)(?:"([^"]+)"|'([^']+)'|([^\\s"']+))`, 'i').exec(command);
  return match ? match[1] || match[2] || match[3] : null;
}

function executable(command) {
  const text = command.trim();
  if (text[0] === '"' || text[0] === "'") return text.slice(1, text.indexOf(text[0], 1));
  // ps does not quote app paths containing spaces. Stop at the language-server
  // executable suffix rather than mistaking a later argument for the executable.
  const server = /^(.+?[\\/]language[_-]server(?:[_-][a-z0-9]+)*(?:\.exe)?)(?=\s|$)/i.exec(text);
  return server?.[1] || text.split(/\s+/)[0];
}

function processInfo(pid, command, executablePath) {
  if (!validPid(pid) || typeof command !== 'string' || command.length > 65536) return null;
  const file = executablePath || executable(command);
  const normalized = file.replace(/\\/g, '/').toLowerCase();
  const basename = normalized.split('/').pop();
  const languageServer = /^language[_-]server(?:[_-][a-z0-9]+)*(?:\.exe)?$/.test(basename);
  const appName = flag(command, '--app_data_dir');
  const override = flag(command, '--override_ide_name');
  const branded = /(?:^|\/)antigravity(?: ide)?(?:\.app)?(?:\/|$)/.test(normalized)
    || /\/extensions\/antigravity\/bin\//.test(normalized)
    || /^antigravity(?:-ide)?$/i.test(appName || '') || /^antigravity$/i.test(override || '');
  const cli = /^(agy|antigravity[_-]cli)(?:\.exe)?$/.test(basename)
    || languageServer && /(?:^|\/)antigravity[_-]cli(?:\/|$)/.test(normalized);
  if (!(languageServer && branded) && !cli) return null;
  const csrfToken = flag(command, '--csrf_token');
  if (csrfToken && !/^[A-Za-z0-9._~+/=-]{1,4096}$/.test(csrfToken)) return null;
  if (!cli && !csrfToken) return null;
  return { pid, csrfToken: csrfToken || null, kind: cli ? 'cli' : 'ide' };
}

function parseProcesses(output, platform) {
  let candidates;
  if (platform === 'win32') {
    let parsed;
    try { parsed = JSON.parse(output || '[]'); } catch { return []; }
    candidates = (Array.isArray(parsed) ? parsed : [parsed]).slice(0, 64)
      .map(row => processInfo(Number(row?.ProcessId), row?.CommandLine, row?.ExecutablePath));
  } else {
    candidates = output.split('\n').slice(0, 16384).map(line => {
      const match = /^\s*(\d+)\s+(.+)$/.exec(line);
      return match ? processInfo(Number(match[1]), match[2]) : null;
    });
  }
  return candidates.filter(Boolean).sort((a, b) => (a.kind === 'cli' ? 1 : 0) - (b.kind === 'cli' ? 1 : 0)
    || b.pid - a.pid).slice(0, 4);
}

function portFromAddress(address) {
  const match = /^(?:127\.0\.0\.1|0\.0\.0\.0|localhost|\*|\[?::1\]?|\[?::\]?):(\d+)$/.exec(address);
  return match && validPort(Number(match[1])) ? Number(match[1]) : null;
}

function parseLsof(output, pid) {
  const ports = new Set();
  for (const line of output.split('\n')) {
    const fields = line.trim().split(/\s+/);
    if (Number(fields[1]) !== pid || !fields.includes('TCP') || fields.at(-1) !== '(LISTEN)') continue;
    const port = portFromAddress(fields.at(-2));
    if (port) ports.add(port);
  }
  return [...ports].slice(0, 64);
}

function parseNetstat(output, pid) {
  const ports = new Set();
  for (const line of output.split('\n')) {
    const fields = line.trim().split(/\s+/);
    if (fields[0] !== 'TCP' || fields[3] !== 'LISTENING' || Number(fields[4]) !== pid) continue;
    const port = portFromAddress(fields[1]);
    if (port) ports.add(port);
  }
  return [...ports].slice(0, 64);
}

async function linuxProcPorts(pid) {
  if (!validPid(pid)) return [];
  const directory = `/proc/${pid}`;
  const inodes = new Set();
  let fds;
  try { fds = (await fs.readdir(path.join(directory, 'fd'))).slice(0, 1024); } catch { return []; }
  // Bound parallel reads and table sizes. Only this PID's owned socket inodes match.
  for (let i = 0; i < fds.length; i += 32) {
    const links = await Promise.all(fds.slice(i, i + 32).map(fd => fs.readlink(path.join(directory, 'fd', fd)).catch(() => '')));
    for (const link of links) {
      const match = /^socket:\[(\d+)\]$/.exec(link);
      if (match) inodes.add(match[1]);
    }
  }
  const tables = await Promise.all(['tcp', 'tcp6'].map(name => readText(path.join(directory, 'net', name), MAX_BODY).catch(() => '')));
  const ports = new Set();
  for (const table of tables) {
    for (const line of table.split('\n').slice(0, 16384)) {
      const fields = line.trim().split(/\s+/);
      if (fields[3] !== '0A' || !inodes.has(fields[9])) continue;
      const port = Number.parseInt(fields[1]?.split(':')[1], 16);
      if (validPort(port)) ports.add(port);
    }
  }
  return [...ports].slice(0, 64);
}

async function discover(options) {
  const platform = options.platform || process.platform;
  let output;
  if (platform === 'win32') {
    output = await runFile(options.execFile, 'powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', WINDOWS_PROCESSES]);
  } else if (platform === 'darwin' || platform === 'linux') {
    const uid = os.userInfo().uid;
    output = await runFile(options.execFile, '/bin/ps', ['-u', String(uid), '-o', 'pid=,args=']);
  } else { throw failure('unsupported-platform'); }
  const processes = parseProcesses(output, platform);
  if (!processes.length) throw failure('not-running');
  const endpoints = [];
  let missingPortsCommand = false;
  const windowsPorts = platform === 'win32'
    ? await runFile(options.execFile, 'netstat.exe', ['-ano', '-p', 'TCP']) : null;
  for (const process of processes) {
    let ports;
    if (windowsPorts !== null) ports = parseNetstat(windowsPorts, process.pid);
    else {
      try {
        const lsof = platform === 'darwin' ? '/usr/sbin/lsof' : 'lsof';
        ports = parseLsof(await runFile(options.execFile, lsof,
          ['-nP', '-a', '-p', String(process.pid), '-iTCP', '-sTCP:LISTEN']), process.pid);
      } catch (error) { missingPortsCommand ||= error.code === 'missing-command'; ports = []; }
      if (!ports.length && platform === 'linux' && !options.execFile) ports = await linuxProcPorts(process.pid);
    }
    for (const port of ports) endpoints.push({ ...process, port });
  }
  if (!endpoints.length) throw failure(missingPortsCommand ? 'missing-port-command' : 'no-ports');
  return endpoints.slice(0, 64);
}

async function rpcRequest(endpoint, method, body, options = {}, token = false, signal) {
  if (!validPort(endpoint.port) || !validPid(endpoint.pid) || !['https', 'http'].includes(endpoint.scheme)
      || ![QUOTA_METHOD, STATUS_METHOD].includes(method)) throw failure('invalid-endpoint');
  const payload = JSON.stringify(body);
  const headers = { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(payload),
    'Connect-Protocol-Version': '1' };
  if (token && endpoint.csrfToken) headers['X-Codeium-Csrf-Token'] = endpoint.csrfToken;
  if (options.fetch) {
    return fetchJson(options.fetch, `${endpoint.scheme}://127.0.0.1:${endpoint.port}${method}`,
      { method: 'POST', headers, body: payload }, RPC_TIMEOUT_MS);
  }
  return new Promise((resolve, reject) => {
    let request;
    let settled = false;
    const finish = (error, result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener('abort', abort);
      error ? reject(failure(error)) : resolve(result);
    };
    const abort = () => { request?.destroy(); finish('network-failed'); };
    const timer = setTimeout(abort, RPC_TIMEOUT_MS);
    if (signal?.aborted) return abort();
    signal?.addEventListener('abort', abort, { once: true });
    const transport = endpoint.scheme === 'https' ? https : http;
    request = transport.request({ hostname: '127.0.0.1', port: endpoint.port, path: method,
      method: 'POST', headers, rejectUnauthorized: false, timeout: RPC_TIMEOUT_MS }, response => {
      const contentType = String(response.headers['content-type'] || '');
      if (!/^application\/(?:json|connect\+json)(?:;|$)/i.test(contentType)
          || Number(response.headers['content-length']) > MAX_BODY) {
        response.destroy();
        request.destroy();
        return finish('not-rpc');
      }
      const chunks = [];
      let size = 0;
      response.on('data', chunk => {
        size += chunk.length;
        if (size > MAX_BODY) { response.destroy(); request.destroy(); finish('too-large'); }
        else chunks.push(chunk);
      });
      response.on('error', () => finish('network-failed'));
      response.on('end', () => {
        let parsed;
        try { parsed = JSON.parse(Buffer.concat(chunks, size).toString('utf8')); }
        catch { return finish('not-rpc'); }
        finish(null, { status: response.statusCode, contentType, body: parsed,
          retryAfter: typeof response.headers['retry-after'] === 'string' ? response.headers['retry-after'] : null });
      });
    });
    request.on('error', () => finish('network-failed'));
    request.on('timeout', abort);
    request.end(payload);
  });
}

function isRpcProof(response, method) {
  if (!/^application\/(?:json|connect\+json)(?:;|$)/i.test(response.contentType)) return false;
  if (response.status === 200) return method === STATUS_METHOD
    ? parseStatus(response.body) !== null : parseQuota(response.body).length > 0;
  const body = response.body;
  // An arbitrary JSON web app is insufficient. Require this service's explicit
  // CSRF challenge in a Connect error before disclosing the process token.
  return typeof body?.code === 'string' && ['unauthenticated', 'permission_denied', 'unknown'].includes(body.code)
    && typeof body.message === 'string' && /\bcsrf\b/i.test(body.message)
    && /missing|required|invalid|token/i.test(body.message);
}

async function resolveEndpoint(endpoints, options) {
  const candidates = endpoints.flatMap(endpoint => ['https', 'http'].map(scheme => ({ ...endpoint, scheme })));
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), DISCOVERY_MS);
  let index = 0;
  let selected = null;
  const worker = async () => {
    while (!controller.signal.aborted && !selected && index < candidates.length) {
      const candidate = candidates[index++];
      for (const method of [QUOTA_METHOD, STATUS_METHOD]) {
        if (controller.signal.aborted || selected) break;
        try {
          const result = await rpcRequest(candidate, method, {}, options, false, controller.signal);
          if (isRpcProof(result, method)) { selected = candidate; controller.abort(); return; }
        } catch { }
      }
    }
  };
  try {
    await Promise.all(Array.from({ length: Math.min(8, candidates.length) }, worker));
    if (!selected) throw failure('not-rpc');
    return selected;
  } finally { clearTimeout(timeout); controller.abort(); }
}

module.exports = {
  discover, resolveEndpoint, rpcRequest, QUOTA_METHOD, STATUS_METHOD, WINDOWS_PROCESSES,
  parseProcesses, processInfo, parseLsof, parseNetstat, isRpcProof, linuxProcPorts
};

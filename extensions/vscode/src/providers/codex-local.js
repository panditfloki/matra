'use strict';

const path = require('node:path');
const { number, count, clean, opaque, epoch, projectLabel, recentFiles, readJsonl, scanBudget, startOfDay } = require('./codex-files');

function tokens(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const input = count(raw.input_tokens);
  const output = count(raw.output_tokens);
  const cached = count(raw.cached_input_tokens);
  // Cached input and reasoning output are subsets, not extra tokens to add.
  const total = count(raw.total_tokens) ?? (input !== null && output !== null ? input + output : null);
  return total === null ? null : { input, output, cached, total };
}

function durationLabel(raw, secondary) {
  const minutes = number(raw.window_minutes) ?? (number(raw.limit_window_seconds) !== null ? raw.limit_window_seconds / 60 : null);
  if (minutes === null || minutes <= 0) return secondary ? 'Longer window' : 'Current session';
  if (minutes === 10080) return 'Weekly limit';
  if (minutes === 43200) return 'Monthly limit';
  if (minutes >= 1440) return `${Math.round(minutes / 1440)}d limit`;
  if (minutes >= 60) return `${Math.round(minutes / 60 * 10) / 10}h limit`;
  return `${Math.round(minutes * 10) / 10}m limit`;
}

function quotaWindows(raw, observedAt) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return [];
  const group = clean(raw.limit_id || raw.limit_name || 'codex');
  const main = group.toLowerCase() === 'codex';
  const groupID = main ? '' : `${opaque(group).slice(0, 12)}-`;
  const windows = [];
  for (const [field, id] of [['primary', 'session'], ['secondary', 'weekly']]) {
    const item = raw[field] ?? raw[`${field}_window`];
    if (!item || number(item.used_percent) === null) continue;
    const reset = epoch(item.resets_at ?? item.reset_at)
      ?? (number(item.reset_after_seconds) !== null ? observedAt + item.reset_after_seconds * 1000 : null);
    windows.push({ id: `${groupID}${id}`, label: `${main ? '' : `${group}: `}${durationLabel(item, field === 'secondary')}`,
      usedPercent: item.used_percent, resetsAt: reset });
  }
  return windows;
}

function explicitAccount(row, session) {
  return row.payload?.rate_limits?.account_id || row.payload?.account_id || row.payload?.user_id || session.accountID || null;
}

async function scanLocal(root, auth, boundary, now) {
  const budget = scanBudget();
  const found = await recentFiles([path.join(root, 'sessions'), path.join(root, 'archived_sessions')], budget);
  const groups = new Map();
  for (const item of found.files) {
    const parsed = { id: opaque(item.file), label: 'Local project', model: 'Unknown model', createdAt: null, accountID: null, events: [], full: true };
    await readJsonl(item, budget, (row, meta) => {
      parsed.full = parsed.full && meta.full;
      if (row.type === 'session_meta') {
        if (typeof row.payload?.id === 'string') parsed.id = row.payload.id;
        else if (typeof row.payload?.session_id === 'string') parsed.id = row.payload.session_id;
        parsed.label = projectLabel(row.payload?.cwd);
        parsed.createdAt = epoch(row.timestamp ?? row.payload?.timestamp);
        parsed.accountID = row.payload?.account_id || row.payload?.user_id || null;
      } else if (row.type === 'turn_context') {
        parsed.model = clean(row.payload?.model) || parsed.model;
      } else if (row.type === 'event_msg' && row.payload?.type === 'token_count') {
        const timestamp = epoch(row.timestamp);
        if (timestamp === null || timestamp > now) return;
        parsed.events.push({ timestamp, totals: tokens(row.payload.info?.total_token_usage),
          rateLimits: row.payload.rate_limits, accountID: explicitAccount(row, parsed) });
      }
    });
    let group = groups.get(parsed.id);
    if (!group) { group = parsed; groups.set(parsed.id, group); }
    else {
      group.events.push(...parsed.events);
      group.full = group.full || parsed.full;
      if (parsed.createdAt !== null && (group.createdAt === null || parsed.createdAt < group.createdAt)) group.createdAt = parsed.createdAt;
    }
    if (Date.now() >= budget.deadline || budget.bytes >= 32 * 1024 * 1024) { budget.partial = true; break; }
  }
  const today = startOfDay(now);
  const daily = [];
  const sessions = [];
  const quotas = new Map();
  let identityPartial = boundary > today;
  let lastUsageAt = null;
  for (const [id, session] of groups) {
    session.events.sort((a, b) => a.timestamp - b.timestamp);
    let previous = null;
    let eligibleLatest = null;
    const sessionDeltas = [];
    for (const event of session.events) {
      const tagged = event.accountID !== null;
      const belongs = tagged ? auth.identities.includes(String(event.accountID)) : event.timestamp >= boundary;
      if (tagged && !belongs) continue;
      if (!tagged && !belongs && event.timestamp >= today) identityPartial = true;
      if (belongs) {
        eligibleLatest = event.timestamp;
        if (auth.mode !== 'api' && event.rateLimits && typeof event.rateLimits === 'object') {
          const raw = event.rateLimits;
          const bucket = clean(raw.limit_id || raw.limit_name || 'codex');
          const windows = quotaWindows(raw, event.timestamp);
          if (windows.length && (!quotas.has(bucket) || quotas.get(bucket).updatedAt <= event.timestamp)) {
            quotas.set(bucket, { windows, updatedAt: event.timestamp, plan: clean(raw.plan_type) || null });
          }
        }
      }
      const current = event.totals;
      if (!current) continue;
      let delta = null;
      if (previous === null) {
        // A complete new session has a known zero baseline. An older/truncated
        // file does not: taking its cumulative total would charge earlier days.
        if (session.full && session.createdAt !== null && session.createdAt >= Math.max(today, boundary)) delta = current;
        else if (belongs && event.timestamp >= today) budget.partial = true;
        previous = current;
      } else if (current.total < previous.total) {
        // Compaction/corrections can reduce a counter. Keep the high-water mark
        // so re-emitted values cannot be counted as new usage after that reset.
        if (belongs && event.timestamp >= today) budget.partial = true;
      } else {
        delta = { total: current.total - previous.total };
        for (const field of ['input', 'output', 'cached']) {
          delta[field] = current[field] !== null && previous[field] !== null && current[field] >= previous[field]
            ? current[field] - previous[field] : null;
          if (delta[field] === null && belongs && event.timestamp >= today) budget.partial = true;
        }
        previous = { total: current.total,
          input: current.input === null ? previous.input : Math.max(previous.input ?? 0, current.input),
          output: current.output === null ? previous.output : Math.max(previous.output ?? 0, current.output),
          cached: current.cached === null ? previous.cached : Math.max(previous.cached ?? 0, current.cached) };
      }
      if (delta && belongs) {
        sessionDeltas.push(delta);
        lastUsageAt = Math.max(lastUsageAt || 0, event.timestamp);
        if (event.timestamp >= today) daily.push(delta);
      }
    }
    if (eligibleLatest !== null) sessions.push({ id: opaque(id).slice(0, 24), label: session.label, detail: session.model,
      lastActiveAt: eligibleLatest, state: now - eligibleLatest < 10 * 60_000 ? 'recent' : 'idle',
      tokens: sessionDeltas.length ? sessionDeltas.reduce((total, delta) => total + delta.total, 0) : null });
  }
  const buckets = [...quotas.values()].sort((a, b) => b.updatedAt - a.updatedAt);
  const sum = field => daily.length && daily.every(item => item[field] !== null) ? daily.reduce((total, item) => total + item[field], 0) : null;
  return { usage: daily.length ? { period: budget.partial || identityPartial ? 'Recorded today (partial)' : 'Today (local device)',
    inputTokens: sum('input'), outputTokens: sum('output'), cachedTokens: sum('cached'), totalTokens: sum('total'), costUsd: null } : null,
    sessions: sessions.sort((a, b) => b.lastActiveAt - a.lastActiveAt).slice(0, 20),
    // One provider timestamp represents all displayed buckets. A fresh Spark
    // row must not make an older main quota appear freshly observed.
    quota: buckets.length ? { windows: buckets.flatMap(bucket => bucket.windows), updatedAt: buckets[buckets.length - 1].updatedAt,
      latestAt: buckets[0].updatedAt,
      plan: buckets.find(bucket => bucket.plan)?.plan || null } : null,
    partial: budget.partial, identityPartial, lastUsageAt, readableRoots: found.readableRoots };
}

module.exports = { scanLocal };

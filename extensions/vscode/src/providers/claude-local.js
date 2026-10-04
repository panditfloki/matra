'use strict';

const path = require('node:path');
const { count, clean, opaque, epoch, projectLabel, recentFiles, readJsonl, scanBudget, startOfDay } = require('./codex-files');

async function localUsage(root, now, boundary) {
  const budget = scanBudget();
  const { files } = await recentFiles([path.join(root, 'projects')], budget);
  const messages = new Map();
  const today = startOfDay(now);
  let identityPartial = boundary > today;
  for (const item of files) {
    await readJsonl(item, budget, (row, meta) => {
      if (row.type !== 'assistant' || !row.message?.usage || row.message.model === '<synthetic>') return;
      const timestamp = epoch(row.timestamp);
      if (timestamp === null || timestamp > now) return;
      if (timestamp < boundary) { if (timestamp >= today) identityPartial = true; return; }
      const raw = row.message.usage;
      const input = count(raw.input_tokens);
      const output = count(raw.output_tokens);
      const read = count(raw.cache_read_input_tokens);
      const written = count(raw.cache_creation_input_tokens);
      const cached = read !== null && written !== null ? read + written : null;
      const total = count(raw.total_tokens) ?? (input !== null && output !== null && cached !== null ? input + output + cached : null);
      if ([input, output, cached, total].every(value => value === null)) return;
      const messageId = typeof row.message.id === 'string' && row.message.id ? row.message.id : row.uuid;
      if (!messageId) budget.partial = true;
      const key = messageId ? `${messageId}:${row.requestId || ''}` : `${item.file}:${meta.index}`;
      const value = { timestamp, input, output, cached, total,
        session: typeof row.sessionId === 'string' ? row.sessionId : item.file,
        label: projectLabel(row.cwd), model: clean(row.message.model) || 'Unknown model' };
      const previous = messages.get(key);
      // Streaming and resumed copies repeat the same message. The most complete
      // usage row replaces earlier copies, rather than counting it again.
      if (!previous || (total ?? output ?? -1) > (previous.total ?? previous.output ?? -1)
        || ((total ?? output) === (previous.total ?? previous.output) && timestamp > previous.timestamp)) messages.set(key, value);
    });
    if (Date.now() >= budget.deadline || budget.bytes >= 32 * 1024 * 1024) { budget.partial = true; break; }
  }
  const entries = [...messages.values()];
  const todayEntries = entries.filter(row => row.timestamp >= today);
  const sum = (rows, field) => rows.length && rows.every(row => row[field] !== null)
    ? rows.reduce((total, row) => total + row[field], 0) : null;
  const groups = new Map();
  for (const row of entries) {
    let session = groups.get(row.session);
    if (!session) { session = { rows: [], latest: row }; groups.set(row.session, session); }
    session.rows.push(row);
    if (row.timestamp > session.latest.timestamp) session.latest = row;
  }
  const partial = budget.partial || todayEntries.some(row => row.total === null);
  return {
    usage: todayEntries.length ? { period: partial || identityPartial ? 'Recorded today (partial)' : 'Today (local device)',
      inputTokens: sum(todayEntries, 'input'), outputTokens: sum(todayEntries, 'output'),
      cachedTokens: sum(todayEntries, 'cached'), totalTokens: sum(todayEntries, 'total'), costUsd: null } : null,
    sessions: [...groups.entries()].map(([id, group]) => ({ id: opaque(id).slice(0, 24),
      label: group.latest.label, detail: group.latest.model, lastActiveAt: group.latest.timestamp,
      state: now - group.latest.timestamp < 10 * 60_000 ? 'recent' : 'idle', tokens: sum(group.rows, 'total') }))
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt).slice(0, 20),
    partial, identityPartial,
  };
}

module.exports = { localUsage };

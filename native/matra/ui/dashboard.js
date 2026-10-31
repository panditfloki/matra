/* The Dashboard: the Mac's DashboardView over the Windows commands. A read-only projection with no
   store and no polling schedule of its own. Readings arrive from the same commands and events as the
   notch; local activity is read on open, on Refresh, on a range change, and on focus after 5 minutes.

   Three rules it must keep:
   - A value the backend did not report stays unknown. Nothing here is estimated or zero-filled.
   - Percentages from different sources are never added together.
   - Local log activity and account quota are separate readings and are labelled so. */
((root) => {
  const STALE_MS = 15 * 60 * 1000;           // UsageStore.staleAfter on the Mac, staleOf in notch.html
  const REPORT_FOCUS_MS = 5 * 60 * 1000;
  const REFRESH_WAIT_MS = 20000;
  const EXTRA = { copilot: ['GitHub Copilot', 'Cp'], opencode: ['OpenCode', 'Oc'], commandcode: ['Command Code', 'Cc'], kimi: ['Kimi', 'K'] };
  const LEGACY = [['codex', 'Codex', 'Cx'], ['glm', 'z.ai', 'Z'], ['cursor', 'Cursor', 'Cu'], ['grok', 'Grok', 'Gk'], ['gemini', 'Antigravity', 'Ag']];
  const RANGES = { day: ['Day', 'Today'], week: ['Week', 'This week'], month: ['Month', 'This month'] };

  const esc = v => String(v == null ? '' : v).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  function bounded(value, limit = 72) {
    const chars = Array.from(String(value == null ? '' : value).replace(/[\u0000-\u001f\u007f]/g, ' ').replace(/\s+/g, ' ').trim());
    return chars.length > limit ? chars.slice(0, limit - 1).join('') + '…' : chars.join('');
  }
  const absent = snap => !snap || snap.status === 'absent';
  const withWindows = snap => ({ ...snap, windows: Array.isArray(snap.windows) ? snap.windows : [] });

  /* ---------- the source list ---------- */
  /* One cell per Claude account, as notch.html claudeCells: each account's windows carry its heading in
     `group` and, past the default account, an `@slug` id. */
  function claudeCells(usage) {
    if (absent(usage)) return [];
    const snap = withWindows(usage), accounts = [];
    for (const w of snap.windows) {
      const g = w.group || '';
      if (!accounts.some(a => a.g === g)) accounts.push({ g, slug: String(w.id).split('@')[1] || '' });
    }
    if (accounts.length < 2) return [{ id: 'claude', base: 'claude', name: 'Claude', letter: 'C', snap }];
    return accounts.map(a => ({
      id: a.slug ? `claude@${a.slug}` : 'claude', base: 'claude', name: a.g || 'Claude', letter: 'C',
      snap: { ...snap, windows: snap.windows.filter(w => (w.group || '') === a.g).map(w => ({ ...w, id: String(w.id).split('@')[0] })) }
    }));
  }
  /* A parity id reaches markup from a saved endpoint, so it is checked rather than trusted. */
  function parityCells(map) {
    const out = [];
    for (const [id, snap] of Object.entries(map || {})) {
      if (absent(snap) || !/^[\w:.@-]+$/.test(id)) continue;
      const name = snap.display_name || id;
      out.push({ id, base: id, name, letter: String(name).replace(/[^\p{L}\p{N}]/gu, '').slice(0, 2) || '?', snap: withWindows(snap) });
    }
    return out;
  }
  function buildSources(data) {
    const list = claudeCells(data.usage);
    for (const [id, name, letter] of LEGACY) if (!absent(data[id])) list.push({ id, base: id, name, letter, snap: withWindows(data[id]) });
    for (const [id, [name, letter]] of Object.entries(EXTRA)) {
      const snap = data.extra && data.extra[id];
      if (!absent(snap)) list.push({ id, base: id, name, letter, snap: withWindows(snap) });
    }
    return list.concat(parityCells(data.parity));
  }

  /* The window a source leads with: the rule in notch.html headlineOf and main.rs ring_window. It only
     decides the order here. No card adds or compares across sources. */
  function tightestOf(ws) { return ws.filter(w => w.count == null).reduce((a, b) => (!a || b.used > a.used || (b.used === a.used && b.id < a.id) ? b : a), null); }
  const laneFamily = w => { const id = String(w.id).toLowerCase(); return id.startsWith('gemini') ? 'gemini' : (id.startsWith('3p') || id.startsWith('claude')) ? '3p' : ''; };
  function laneIs(w, limit) {
    const t = `${w.id} ${w.label}`.toLowerCase();
    return limit === 'weekly' ? t.includes('weekly') : ['5h', '5-hour', 'five hour', 'five-hour', 'hourly', 'session'].some(k => t.includes(k));
  }
  function headlineOf(snap, id, ag) {
    const ws = snap.windows; if (!ws.length) return null;
    const byId = x => ws.find(w => w.id === x) || null;
    if (id === 'claude' || id === 'glm') return byId('session');
    if (id === 'codex') return byId('primary');
    if (id === 'cursor') return byId('included') || byId('api');
    if (id === 'grok') return byId('credits') || ws[0];
    if (id === 'copilot') return byId('premium_interactions') || ws[0];
    if (id === 'opencode' || id === 'kimi') return byId('rolling');
    if (id === 'commandcode') return byId('monthly');
    if (id !== 'gemini') return tightestOf(ws.filter(w => w.used < 1)) || tightestOf(ws) || ws[0];
    const prefs = ag || { limit: 'automatic', model: 'gemini' };
    const fam = ws.filter(w => laneFamily(w) === prefs.model), lanes = fam.length ? fam : ws;
    if (prefs.limit !== 'automatic') { const w = tightestOf(lanes.filter(w => laneIs(w, prefs.limit))); if (w) return w; }
    return tightestOf(lanes.filter(w => w.used < 1)) || tightestOf(lanes) || lanes[0];
  }
  function weeklyOf(snap, id, ag) {
    const ws = snap.windows.filter(w => w.count == null); if (!ws.length) return null;
    const byId = x => ws.find(w => w.id === x) || null;
    if (id === 'claude') return byId('seven_day') || byId('weekly_all') || byId('weekly');
    if (id === 'codex') return byId('secondary');
    if (id === 'cursor' || id === 'grok' || id === 'copilot') return null;
    if (id === 'glm' || ['opencode', 'commandcode', 'kimi'].includes(id)) return byId('weekly');
    if (id !== 'gemini') return byId('weekly') || byId('seven_day');
    const prefs = ag || { limit: 'automatic', model: 'gemini' };
    const fam = ws.filter(w => laneFamily(w) === prefs.model), lanes = fam.length ? fam : ws;
    return tightestOf(lanes.filter(w => laneIs(w, 'weekly')));
  }
  function staleOf(snap, now) { return snap.status === 'stale' || (snap.fetched_at > 0 && now - snap.fetched_at > STALE_MS); }
  /* The notch's order, without its hiding: a Dashboard lists every source that has a reading. The chosen
     slots lead, in their own order. A source left off the notch still appears after them. */
  function orderSources(list, opts = {}) {
    const slots = Array.isArray(opts.slots) ? opts.slots : [];
    const slotted = slots.flatMap(s => list.filter(p => s && p.base === s.provider));
    const manual = slotted.concat(list.filter(p => !slotted.includes(p)));
    const parity = root.MatraParity;
    if (!parity) return manual;
    const now = opts.now || Date.now();
    return parity.order(manual, parity.normalize(opts.parity).ring_order, {
      headline: p => headlineOf(p.snap, p.base, opts.ag),
      weekly: p => weeklyOf(p.snap, p.base, opts.ag),
      fresh: p => p.snap.status === 'ok' && !staleOf(p.snap, now)
    }, now);
  }

  /* ---------- wording ---------- */
  function smallPct(v) {
    if (v <= 0) return '0';
    const t = Math.round(v * 10) / 10;
    if (t < 0.1) return '<0.1';
    if (t > 99.9) return '>99.9';
    return t.toFixed(1);
  }
  /* Both ends, because vendors disagree on which they print. */
  function usedText(w) {
    const v = w.used * 100;
    let used, left;
    if ((v > 0 && v < 1) || (v > 99 && v < 100)) { used = smallPct(v); left = 100 - v > 99.9 ? '>99.9' : smallPct(Math.max(0, 100 - v)); }
    else { const u = Math.round(v); used = String(u); left = String(Math.max(0, 100 - u)); }
    return `${w.derived ? '~' : ''}${used}% used · ${left}% left`;
  }
  const dayStart = ms => { const d = new Date(ms); return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime(); };
  function clockText(ms, clock24) {
    const options = { hour: 'numeric', minute: '2-digit' };
    if (clock24 === true) options.hourCycle = 'h23';
    if (clock24 === false) options.hourCycle = 'h12';
    return new Date(ms).toLocaleTimeString(undefined, options);
  }
  function resetText(ms, now, clock24) {
    if (!Number.isFinite(ms) || ms <= 0) return '';
    const diff = ms - now;
    if (diff <= 0) return 'Resetting…';
    const min = Math.round(diff / 60000);
    if (min < 60) return `Resets in ${Math.max(1, min)} min`;
    const d = new Date(ms);
    // A weekday only names a day in the coming week.
    if (Math.round((dayStart(ms) - dayStart(now)) / 86400000) >= 7) return `Resets ${d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}`;
    if (diff < 86400000) return `Resets at ${clockText(ms, clock24)}`;
    return `Resets ${d.toLocaleDateString(undefined, { weekday: 'short' })} ${clockText(ms, clock24)}`;
  }
  function agoText(ms, now) {
    const min = Math.round((now - ms) / 60000);
    if (min < 1) return 'just now';
    if (min < 60) return `${min} min ago`;
    const hr = Math.round(min / 60);
    return hr < 24 ? `${hr} hr ago` : `${Math.round(hr / 24)} d ago`;
  }
  function statusView(snap, now, clock24) {
    if (snap.status === 'needsAuth') return { tone: 'bad', label: 'Sign-in needed' };
    if (snap.status === 'backoff' || snap.backoff_until > now) {
      return { tone: 'watch', label: snap.backoff_until > now ? `Waiting to retry until ${clockText(snap.backoff_until, clock24)}` : 'Waiting to retry' };
    }
    if (snap.status === 'error') return { tone: 'bad', label: snap.fetched_at ? `Last read failed · updated ${agoText(snap.fetched_at, now)}` : 'Last read failed' };
    if (!snap.fetched_at) return { tone: 'muted', label: snap.windows.length ? 'Time of reading unknown' : 'Waiting for first reading' };
    if (staleOf(snap, now)) return { tone: 'watch', label: `Stale · updated ${agoText(snap.fetched_at, now)}` };
    return { tone: 'good', label: `Updated ${agoText(snap.fetched_at, now)}` };
  }
  function sourceNote(source) {
    const snap = source.snap;
    if (snap.kind === 'localRuntime') return 'Measured from a local runtime. This is not account quota.';
    if (String(source.base).startsWith('custom:')) return 'Read from your custom endpoint. A budget you entered is your figure, not a provider bill.';
    if (snap.windows.some(w => w.derived)) return 'Derived reading, not a provider bill. Unavailable values stay unknown.';
    return 'Provider-reported reading. Unavailable values stay unknown.';
  }
  const SIGN_IN = { claude: 'Sign in to Claude Code to see usage.', cursor: 'Sign in to Cursor to see usage.', codex: 'Sign in to Codex to see usage.', grok: 'Run grok login to see usage.', gemini: 'Sign in to Antigravity to see usage.' };

  /* ---------- glyphs ---------- */
  /* An SVG is drawn as a CSS mask, never as markup: an image cannot run script, whatever a user-dropped
     file contains, and the mask still takes the theme's text colour. */
  function glyphHtml(source, glyphs) {
    const g = glyphs && glyphs[source.base];
    if (g && g.kind === 'svg' && typeof g.svg === 'string' && /<svg[\s>]/i.test(g.svg)) {
      const svg = /xmlns=/.test(g.svg) ? g.svg : g.svg.replace(/<svg/i, '<svg xmlns="http://www.w3.org/2000/svg"');
      const url = `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
      return `<span class="dash-glyph mask" aria-hidden="true" style="${esc(`-webkit-mask-image:${url};mask-image:${url}`)}"></span>`;
    }
    if (g && typeof g.url === 'string' && /^data:image\/[a-z0-9.+-]+[;,]/i.test(g.url)) return `<span class="dash-glyph"><img src="${esc(g.url)}" alt=""></span>`;
    return `<span class="dash-glyph letter" aria-hidden="true">${esc(source.letter)}</span>`;
  }

  /* ---------- quota windows ---------- */
  function windowHtml(w, source, ctx) {
    const texts = source.snap.details && source.snap.details.texts && source.snap.details.texts[w.id];
    const label = `<span class="dash-win-label">${esc(w.label || w.id)}</span>`;
    if (w.count != null) {
      // Only Antigravity's counts are requests today. A source that reports its own kind names the count in its label.
      const value = texts && texts.used_text ? texts.used_text : source.snap.kind ? String(w.count) : w.count > 0 ? `~${w.count} ${w.count === 1 ? 'request today' : 'requests today'}` : 'No requests today';
      return `<div class="dash-win"><div class="dash-win-top">${label}</div><div class="dash-win-value"><strong>${esc(value)}</strong>${texts && texts.detail ? ` · ${esc(texts.detail)}` : ''}</div></div>`;
    }
    if (!Number.isFinite(w.used) || w.used < 0) return `<div class="dash-win"><div class="dash-win-top">${label}</div><div class="dash-win-value">Not reported</div></div>`;
    const reset = resetText(w.resets_at, ctx.now, ctx.clock24) || 'Reset time not reported';
    const remaining = root.MatraParity ? root.MatraParity.normalize(ctx.parity).shows_remaining : false;
    const shown = remaining ? Math.max(0, 1 - w.used) : w.used;
    const tone = root.MatraQuota ? root.MatraQuota.tone(w.used, ctx.quota) : 'var(--matra-accent)';
    const words = usedText(w);
    const value = texts && (texts.detail || texts.used_text) ? `${words} · ${texts.detail || texts.used_text}` : words;
    return `<div class="dash-win"><div class="dash-win-top">${label}<span class="dash-win-reset">${esc(reset)}</span></div>`
      + `<div class="dash-track" role="progressbar" aria-label="${esc(w.label || w.id)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(Math.min(shown, 1) * 100)}" aria-valuetext="${esc(words)}"><i style="width:${(Math.min(shown, 1) * 100).toFixed(1)}%;background:${esc(tone)}"></i></div>`
      + `<div class="dash-win-value">${esc(value)}</div></div>`;
  }
  /* Signed out shows no cached windows: a disconnected account's old figures are not a reading. */
  function windowsHtml(source, ctx) {
    const snap = source.snap;
    if (snap.status === 'needsAuth') {
      const parts = [SIGN_IN[source.base], snap.note].filter(Boolean);
      return `<p class="dash-note">${esc(parts.join(' ') || 'Sign in to this source to see usage.')}</p>`;
    }
    if (!snap.windows.length) return `<p class="dash-note">${esc(snap.note || 'Waiting for first reading…')}</p>`;
    const blocks = []; let current = null;
    for (const w of snap.windows) {
      const name = w.group && w.group !== source.name ? w.group : null;
      if (!current || current.name !== name) { current = { name, rows: [] }; blocks.push(current); }
      current.rows.push(windowHtml(w, source, ctx));
    }
    const body = blocks.map(b => (b.name ? `<div class="dash-group"><h4>${esc(b.name)}</h4><div class="dash-group-box">${b.rows.join('')}</div></div>` : b.rows.join(''))).join('');
    return `<div class="dash-windows">${body}</div>${snap.note ? `<p class="dash-note">${esc(snap.note)}</p>` : ''}`;
  }
  function pillsHtml(source, ctx) {
    const view = statusView(source.snap, ctx.now, ctx.clock24);
    const kind = source.snap.kind === 'localRuntime' ? '<span class="dash-pill">Local runtime</span>' : '';
    return `<span class="dash-pills">${kind}<span class="dash-pill ${view.tone}"><i></i>${esc(view.label)}</span></span>`;
  }
  function cardHeadHtml(source, ctx, heading) {
    const plan = source.snap.kind ? '' : source.snap.codex_details && source.snap.codex_details.plan;
    return `<div class="dash-card-head">${glyphHtml(source, ctx.glyphs)}<${heading}>${esc(source.name)}</${heading}>${plan ? `<span class="dash-plan">${esc(bounded(plan, 64))}</span>` : ''}${pillsHtml(source, ctx)}</div>`;
  }

  /* ---------- local activity (get_activity_report) ---------- */
  const familyOf = base => (base === 'claude' || base === 'codex' ? base : null);
  const inFamily = (account, family) => account === family || String(account).startsWith(`${family}-`);
  const rowsFor = (report, family) => (report && Array.isArray(report.rows) ? report.rows : []).filter(r => !family || inFamily(r.account, family));
  function digest(rows) {
    const A = root.MatraActivity;
    const whole = A.groupBy(rows, () => 'all', () => 'all')[0];
    return { tokens: whole ? whole.tokens : 0, cost: whole ? A.groupCost(whole).text : '-' };
  }
  function periodText(report) {
    if (!report || !report.from) return '';
    return report.from === report.to ? report.from : `${report.from} to ${report.to}`;
  }
  function rangeSegHtml(ctx) {
    const buttons = Object.entries(RANGES).map(([id, [word]]) => `<button type="button" data-act="range" data-v="${id}" data-k="range:${id}" aria-pressed="${ctx.range === id}">${word}</button>`).join('');
    return `<span class="seg" role="group" aria-label="Local activity period">${buttons}</span>`;
  }
  function chartHtml(key, cells, readout, hint) {
    const n = cells.length;
    return `<div class="act-chart" style="grid-template-columns:repeat(${n},minmax(0,1fr))">${cells.map(c => c.html).join('')}</div>`
      + `<div class="act-axis" aria-hidden="true"><span>${esc(cells[0].from)}</span><span>${esc(cells[n - 1].from)}</span></div>`
      + `<div class="act-readout" data-readout-for="${esc(key)}" role="status" aria-live="polite">${esc(readout || hint)}</div>`;
  }
  function dayChartHtml(key, report, rows, ctx) {
    const A = root.MatraActivity;
    const bars = A.barScale(A.dayBars({ ...report, rows }), ctx.measure);
    if (bars.length < 2) return '';
    const cells = bars.map(b => {
      const label = `${b.id}: ${A.count(b.group.tokens)} tokens, ${A.groupCost(b.group).text}`;
      return { from: b.id, html: `<button type="button" class="act-bar${b.unknown ? ' unknown' : ''}" data-chart="${esc(key)}" data-readout="${esc(label)}" data-k="bar:${esc(key)}:${esc(b.id)}" aria-label="${esc(label)}"><i style="height:${b.unknown ? 4 : b.height}%"></i></button>` };
    });
    const measure = ['tokens', 'cost'].map(m => `<button type="button" data-act="measure" data-v="${m}" data-k="measure:${esc(key)}:${m}" aria-pressed="${ctx.measure === m}">${m === 'cost' ? 'Cost' : 'Tokens'}</button>`).join('');
    return `<div class="item"><span>By day</span><span class="seg" role="group" aria-label="Chart measure">${measure}</span></div>`
      + `<div class="dash-chart">${chartHtml(key, cells, ctx.readouts && ctx.readouts[key], 'Hover or focus a bar to read that day.')}</div>`
      + (ctx.measure === 'cost' && bars.some(b => b.unknown) ? '<div class="item cap">Short dashed marks mean the cost is unavailable, not zero.</div>' : '');
  }
  function listHtml(title, rows, id, name, limit, sticky) {
    const A = root.MatraActivity;
    const groups = A.groupBy(rows, id, name);
    if (!groups.length) return '';
    const items = groups.slice(0, limit).map(g => `<div class="act-row"><span class="act-name">${esc(g.name)}</span><span class="act-tok">${esc(A.count(g.tokens))} tokens</span><span class="act-cost">${esc(A.groupCost(g).text)}</span></div>`).join('');
    const more = groups.length > limit ? `<div class="item cap">Showing the top ${limit} of ${groups.length}. ${esc(sticky)}</div>` : '';
    return `<div><h2 class="dash-h">${esc(title)}</h2><div class="group dash-gapped">${items}${more}</div></div>`;
  }
  function sessionsHtml(report, family) {
    const A = root.MatraActivity;
    const list = (Array.isArray(report.sessions) ? report.sessions : []).filter(s => !family || inFamily(s.account, family));
    if (!list.length) return '';
    const items = list.slice(0, 8).map(s => {
      const cost = Number.isFinite(s.cost_inr) ? A.money(s.cost_inr, 'complete') : s.tokens ? 'Unavailable' : '-';
      const when = Number.isFinite(s.last_ts) && s.last_ts > 0 ? new Date(s.last_ts * (s.last_ts < 1e11 ? 1000 : 1)).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '';
      const mins = Number.isFinite(s.duration_seconds) && s.duration_seconds >= 0 ? `${Math.max(1, Math.round(s.duration_seconds / 60))} min` : '';
      const models = Array.isArray(s.models) ? s.models.join(', ') : '';
      return `<div class="act-row"><span class="act-name">${esc(s.project_name || 'Unknown project')}<small>${esc([when, mins, models].filter(Boolean).join(' · '))}</small></span><span class="act-tok">${esc(A.count(s.tokens))} tokens</span><span class="act-cost">${esc(cost)}</span></div>`;
    }).join('');
    return `<div><h2 class="dash-h">Recent sessions</h2><div class="group dash-gapped">${items}</div></div>`;
  }
  function reportNotesHtml(report) {
    const notes = [];
    if (report.note) notes.push(report.note);
    if (report.partial) notes.push('Partial report: some turns could not be priced, or some log lines could not be read.');
    if (Number(report.unpriced_turns) > 0) notes.push(`${report.unpriced_turns} turns have no known price and add no cost here.`);
    if (Array.isArray(report.unpriced_models) && report.unpriced_models.length) notes.push(`Unpriced models: ${report.unpriced_models.join(', ')}`);
    if (report.scan && (report.scan.files_skipped || report.scan.lines_skipped)) notes.push(`Skipped while reading logs: ${report.scan.files_skipped || 0} files, ${report.scan.lines_skipped || 0} lines`);
    return notes.length ? `<div class="group">${notes.map(n => `<div class="item cap">${esc(n)}</div>`).join('')}</div>` : '';
  }
  function reportStateHtml(ctx) {
    if (ctx.reportError && !ctx.report) return `<div class="group"><div class="item"><span class="act-error">${esc(ctx.reportError)}</span><button type="button" class="btn" data-act="reload" data-k="report:reload">Try again</button></div></div>`;
    if (!ctx.report) return `<div class="group"><div class="item cap">${ctx.reportBusy ? 'Reading local logs…' : 'No local report loaded yet.'}</div></div>`;
    return '';
  }
  const staleReportHtml = ctx => (ctx.report && ctx.reportError ? `<p class="dash-note dash-warn">Showing the previous read. ${esc(ctx.reportError)}</p>` : '');
  function overviewLocalHtml(ctx) {
    const A = root.MatraActivity;
    const head = `<div class="dash-sechead"><h2 class="dash-h">${esc(RANGES[ctx.range][1])} on this computer</h2>${rangeSegHtml(ctx)}</div>`
      + `<p class="dash-note">Local Claude and Codex logs${ctx.report ? ` · ${esc(periodText(ctx.report))}` : ''}. Account quota and account-wide profile tokens are shown separately and are not added in.</p>`;
    const waiting = reportStateHtml(ctx);
    if (waiting) return head + waiting;
    const report = ctx.report, rows = rowsFor(report);
    if (!rows.length) return `${head}${staleReportHtml(ctx)}<div class="group"><div class="item cap">No Claude or Codex activity was found on this computer for this period.</div></div>${reportNotesHtml(report)}`;
    const tokens = rows.reduce((sum, r) => sum + A.rowTokens(r), 0);
    const api = A.coverageOf(report.total_cost_inr, tokens, report.partial, report.coverage);
    const spend = report.estimated_spend;
    const metric = (label, value, hint) => `<div class="act-metric"><span>${esc(label)}</span><strong>${esc(value)}</strong>${hint ? `<small>${esc(hint)}</small>` : ''}</div>`;
    return head + staleReportHtml(ctx)
      + `<div class="group act-metrics">${metric('Tokens', A.count(tokens))}${spend ? metric('Estimated spend', A.money(spend.amount_inr, spend.coverage), 'Plan share, not a bill') : ''}${metric('API-equivalent (estimate)', A.money(report.total_cost_inr, api), 'Comparison at list price')}</div>`
      + `<div class="group">${dayChartHtml('overview', report, rows, ctx) || '<div class="item cap">One day in this period, so there is no daily chart.</div>'}</div>`
      + listHtml('Top models', rows, r => r.model, r => r.model, 5, 'Project activity has the full view.')
      + reportNotesHtml(report);
  }
  function detailLocalHtml(family, ctx) {
    const head = `<div class="dash-sechead"><h2 class="dash-h">Local device activity</h2>${rangeSegHtml(ctx)}</div>`
      + `<p class="dash-note">From this computer's logs${ctx.report ? ` · ${esc(periodText(ctx.report))}` : ''}. This is not your account quota.</p>`;
    const waiting = reportStateHtml(ctx);
    if (waiting) return head + waiting;
    const report = ctx.report, rows = rowsFor(report, family);
    if (!rows.length) return `${head}${staleReportHtml(ctx)}<div class="group"><div class="item cap">No local activity was found for this source in this period.</div></div>`;
    const A = root.MatraActivity, total = digest(rows);
    const nameOf = r => r.project_name || ((report.projects || []).find(p => p.id === r.project) || {}).name || 'Unknown project';
    const metric = (label, value, hint) => `<div class="act-metric"><span>${esc(label)}</span><strong>${esc(value)}</strong>${hint ? `<small>${esc(hint)}</small>` : ''}</div>`;
    return head + staleReportHtml(ctx)
      + `<div class="group act-metrics">${metric('Tokens', A.count(total.tokens))}${metric('API-equivalent (estimate)', total.cost, 'Comparison at list price')}</div>`
      + `<div class="group">${dayChartHtml(`detail-${family}`, report, rows, ctx) || '<div class="item cap">One day in this period, so there is no daily chart.</div>'}</div>`
      + listHtml('Models', rows, r => r.model, r => r.model, 12, '')
      + listHtml('Projects', rows, r => r.project, nameOf, 12, 'Project activity has the full view.')
      + sessionsHtml(report, family)
      + reportNotesHtml(report);
  }

  /* ---------- provider details ---------- */
  const integer = v => (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : null);
  function fullCount(v) { const n = integer(v); return n === null ? 'Not reported' : new Intl.NumberFormat().format(n); }
  function shortCount(v) { const n = integer(v); return n === null ? 'Not reported' : root.MatraActivity.count(n); }
  function spanText(v) {
    if (typeof v !== 'number' || !Number.isFinite(v) || v < 0) return 'Not reported';
    const s = Math.floor(v), m = Math.floor(s / 60), h = Math.floor(m / 60), d = Math.floor(h / 24);
    if (d) return `${d} d ${h % 24} hr`;
    if (h) return `${h} hr ${m % 60} min`;
    return m ? `${m} min ${s % 60} sec` : `${s} sec`;
  }
  /* Thirty local calendar days. A day with no bucket is unknown, never zero. */
  function historyDays(buckets, now) {
    const reported = new Map();
    if (Array.isArray(buckets)) for (const b of buckets) if (b && typeof b.day === 'string') reported.set(b.day, integer(b.tokens));
    const noon = new Date(now); noon.setHours(12, 0, 0, 0);
    const days = Array.from({ length: 30 }, (_, i) => {
      const d = new Date(noon); d.setDate(d.getDate() - 29 + i);
      const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
      return { key, tokens: reported.has(key) ? reported.get(key) : null };
    });
    const known = days.filter(d => d.tokens !== null);
    return { days, count: known.length, total: known.length ? known.reduce((sum, d) => sum + d.tokens, 0) : null, available: Array.isArray(buckets) };
  }
  function historyHtml(key, title, buckets, ctx) {
    const h = historyDays(buckets, ctx.now);
    if (!h.available) return '';
    const max = Math.max(0, ...h.days.map(d => d.tokens || 0));
    const cells = h.days.map(d => {
      const label = `${d.key}: ${d.tokens === null ? 'Not reported' : `${fullCount(d.tokens)} tokens`}`;
      const height = d.tokens !== null && max > 0 ? Math.max(d.tokens ? 3 : 0, Math.round(d.tokens / max * 100)) : 0;
      return { from: d.key, html: `<button type="button" class="act-bar${d.tokens === null ? ' unknown' : ''}" data-chart="${esc(key)}" data-readout="${esc(label)}" data-k="bar:${esc(key)}:${d.key}" aria-label="${esc(label)}"><i style="height:${d.tokens === null ? 4 : height}%"></i></button>` };
    });
    const summary = h.count ? `${h.count === 30 ? 'All 30 days reported' : `${h.count} of 30 days reported`} · ${shortCount(h.total)} tokens${h.count < 30 ? ' reported' : ''}` : 'No days reported in this window';
    return `<div class="dash-section"><h3 class="dash-chart-title">${esc(title)}</h3><div class="dash-chart">${chartHtml(key, cells, ctx.readouts && ctx.readouts[key], 'Hover or focus a day to read its tokens.')}</div><p class="dash-note">${esc(summary)}. Dashed marks mean not reported. Account-wide and local token totals are not added together.</p></div>`;
  }
  function codexHtml(details, ctx) {
    const d = details && typeof details === 'object' ? details : null;
    if (!d) return '';
    const stats = d.statistics && typeof d.statistics === 'object' ? d.statistics : {};
    const credits = d.credits && typeof d.credits === 'object' ? d.credits : {};
    const days = n => (integer(n) === null ? 'Not reported' : `${fullCount(n)} days`);
    const expiry = integer(credits.next_expiry_ms);
    const expiryText = expiry !== null && expiry > ctx.now ? `Next expiry ${new Date(expiry).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}` : expiry ? 'Reported expiry passed; refresh required.' : 'Expiry not reported';
    const metric = (label, value) => `<div><dt>${esc(label)}</dt><dd>${esc(value)}</dd></div>`;
    const errors = [d.credits_error && `Resets: ${bounded(d.credits_error, 160)}`, d.statistics_error && `Statistics: ${bounded(d.statistics_error, 160)}`].filter(Boolean);
    const buckets = Array.isArray(stats.daily_usage_buckets) ? stats.daily_usage_buckets.map(b => (b ? { day: b.start_date, tokens: b.tokens } : b)) : null;
    return `<div class="dash-section"><h3 class="dash-chart-title">Account usage, reported by Codex</h3><dl class="dash-stats">${metric('Unused resets', fullCount(credits.available_count))}${metric('Lifetime tokens', shortCount(stats.lifetime_tokens))}${metric('Peak daily tokens', shortCount(stats.peak_daily_tokens))}${metric('Longest chat', spanText(stats.longest_running_turn_sec))}${metric('Current streak', days(stats.current_streak_days))}${metric('Longest streak', days(stats.longest_streak_days))}</dl><p class="dash-note">${esc(expiryText)}</p>${errors.map(e => `<p class="dash-note" role="status">${esc(e)}</p>`).join('')}</div>`
      + historyHtml('codex-history', 'Account tokens by day', buckets, ctx);
  }
  function liveHtml(source, ctx) {
    let rows = [];
    if (source.base === 'claude') {
      rows = (ctx.sessions || []).filter(s => s && s.state && s.state !== 'idle').map(s => ({ waiting: s.state === 'attention', name: s.title, state: { running: 'Working', attention: 'Waiting', done: 'Done' }[s.state] || s.state }));
    } else {
      rows = (ctx.activity || []).filter(a => a && a.provider === source.base).sort((a, b) => (b.state === 'waiting') - (a.state === 'waiting') || b.since - a.since).map(a => ({ waiting: a.state === 'waiting', name: a.name, state: a.detail || (a.state === 'waiting' ? 'Waiting' : 'Working') }));
    }
    if (!rows.length) return '';
    const shown = rows.slice(0, 5).map(r => `<div class="dash-live-row${r.waiting ? ' waiting' : ''}"><i aria-hidden="true"></i><span>${esc(bounded(r.name))}</span><span>${esc(bounded(r.state, 24))}</span></div>`).join('');
    return `<div class="dash-live">${shown}${rows.length > 5 ? `<p class="dash-note">and ${rows.length - 5} more</p>` : ''}</div>`;
  }
  function parityDetailsHtml(source) {
    return source.snap.kind && root.MatraParityCard ? root.MatraParityCard.html(source.snap, source.base, esc) : '';
  }

  /* ---------- views ---------- */
  function overviewHtml(sources, ctx) {
    const cards = sources.map(source => {
      const family = familyOf(source.base);
      const rows = family && ctx.report ? rowsFor(ctx.report, family) : [];
      let local = '';
      if (rows.length) {
        const total = digest(rows);
        local = `<div class="dash-local"><b>${esc(root.MatraActivity.count(total.tokens))} tokens</b><b>${esc(total.cost)}</b><span>${esc(RANGES[ctx.range][1])} in local logs, API-equivalent estimate. Not account quota.</span></div>`;
      }
      return `<article class="dash-card" aria-label="${esc(source.name)}">${cardHeadHtml(source, ctx, 'h3')}${windowsHtml(source, ctx)}${parityDetailsHtml(source)}${local}<div class="dash-card-foot"><p class="dash-note">${esc(sourceNote(source))}</p><button type="button" class="btn" data-act="select" data-id="${esc(source.id)}" data-k="open:${esc(source.id)}" aria-label="Open ${esc(source.name)} details">Open details</button></div></article>`;
    }).join('');
    return `${overviewLocalHtml(ctx)}<div class="dash-gapped"><h2 class="dash-h">Current allowances</h2><p class="dash-note" style="margin-top:6px">Each source has its own limits. These percentages are not added together.</p></div>${cards}`;
  }
  function detailHtml(source, ctx) {
    const family = familyOf(source.base);
    const custom = source.snap.details && source.snap.details.usage_history;
    const history = Array.isArray(custom) ? custom.map(b => (b ? { day: b.day, tokens: b.total_tokens } : b)) : null;
    const card = `<article class="dash-card">${cardHeadHtml(source, ctx, 'h2')}${windowsHtml(source, ctx)}${source.base === 'codex' ? codexHtml(source.snap.codex_details, ctx) : ''}${parityDetailsHtml(source)}${history ? historyHtml('custom-history', 'Endpoint tokens by day', history, ctx) : ''}${liveHtml(source, ctx)}</article>`;
    const closing = family
      ? 'Local log totals can include several accounts on this computer. API-price estimates are not subscription bills. Account-wide and local token totals are not added together.'
      : 'No local token logs are read for this source, so nothing here is a token or cost total from this computer.';
    return `${card}${family ? detailLocalHtml(family, ctx) : ''}<p class="dash-note">${esc(sourceNote(source))}</p><p class="dash-note">${esc(closing)}</p>`;
  }
  function emptyHtml() {
    return '<div class="dash-card"><h2 class="dash-h">No readings to show yet</h2><p class="dash-note">Manage your AI sources in Settings. Connected sources appear here when a reading is published.</p><div><button type="button" class="btn" data-act="settings" data-k="empty:settings">Open Settings</button></div></div>';
  }

  /* ---------- the live page ---------- */
  function mount(doc, api) {
    const invoke = api.invoke, listen = api.listen;
    const $ = id => doc.getElementById(id);
    const state = {
      data: { usage: null, codex: null, cursor: null, grok: null, gemini: null, glm: null, extra: null, parity: null },
      pushed: new Set(), failed: new Map(), loaded: false,
      glyphs: {}, slots: null, parity: null, ag: null, quota: null, clock24: undefined, sessions: [], activity: [],
      selected: null, range: 'month', measure: 'tokens', readouts: {},
      report: null, reportError: '', reportBusy: false, reportAt: 0, reportToken: 0,
      refreshing: new Map(), note: '', strip: ''
    };
    const sources = () => orderSources(buildSources(state.data), { slots: state.slots, parity: state.parity, ag: state.ag });
    const ctx = () => ({ now: Date.now(), clock24: state.clock24, glyphs: state.glyphs, parity: state.parity, quota: state.quota, sessions: state.sessions, activity: state.activity, range: state.range, measure: state.measure, readouts: state.readouts, report: state.report, reportError: state.reportError, reportBusy: state.reportBusy });

    let queued = false;
    function schedule() {
      if (queued) return;
      queued = true;
      (root.requestAnimationFrame || (fn => setTimeout(fn, 16)))(() => { queued = false; render(); });
    }
    function render() {
      const list = sources(), c = ctx();
      // A source that disconnected takes its cached details with it.
      if (state.selected && !list.some(s => s.id === state.selected)) state.selected = null;
      const chosen = list.find(s => s.id === state.selected) || null;
      const focused = doc.activeElement && doc.activeElement.dataset ? doc.activeElement.dataset.k : null;
      const scroll = $('dash-scroll'), rail = $('dash-sources');
      const top = scroll.scrollTop, railTop = rail.scrollTop;

      if (chosen) $('dash-overview').removeAttribute('aria-current'); else $('dash-overview').setAttribute('aria-current', 'page');
      rail.innerHTML = list.map(s => `<div role="listitem"><button type="button" class="dash-row" data-act="select" data-id="${esc(s.id)}" data-k="src:${esc(s.id)}"${s.id === state.selected ? ' aria-current="page"' : ''}>${glyphHtml(s, c.glyphs)}<span>${esc(s.name)}</span></button></div>`).join('');
      $('dash-title').textContent = chosen ? chosen.name : 'Overview';
      $('dash-sub').textContent = chosen ? 'The same reading as your side notch.' : 'Your notch readings, with room to explore.';

      const busy = state.refreshing.size > 0 || state.reportBusy;
      $('dash-refresh-label').textContent = busy ? 'Refreshing…' : 'Refresh';
      $('dash-refresh').disabled = busy || !list.length;

      const failures = [...state.failed.keys()];
      const strip = [state.strip, failures.length ? `Could not read: ${failures.join(', ')}. Those sources are left out, not shown as empty.` : '', state.note].filter(Boolean).join(' ');
      $('dash-strip').textContent = strip;
      $('dash-strip').hidden = !strip;

      $('dash-body').innerHTML = !state.loaded ? '<p class="dash-note">Reading sources…</p>' : !list.length ? emptyHtml() : chosen ? detailHtml(chosen, c) : overviewHtml(list, c);
      scroll.scrollTop = top; rail.scrollTop = railTop;
      if (focused) {
        const again = doc.querySelector(`[data-k="${focused.replace(/["\\]/g, '\\$&')}"]`);
        if (again && !again.disabled) again.focus({ preventScroll: true });
      }
    }

    /* ---- reports ---- */
    async function loadReport() {
      const token = ++state.reportToken;
      state.reportBusy = true; schedule();
      try {
        const report = await invoke('get_activity_report', { range: state.range, account: null, project: null, date: null });
        if (token !== state.reportToken) return;
        state.report = report && typeof report === 'object' ? report : null;
        state.reportError = ''; state.reportAt = Date.now();
      } catch (e) {
        if (token !== state.reportToken) return;
        state.reportError = `Could not read local activity: ${bounded((e && e.message) || e, 200)}`;
      }
      state.reportBusy = false; schedule();
    }

    /* ---- refresh ---- */
    function settle(ids) { let hit = false; for (const id of ids) { const t = state.refreshing.get(id); if (t) { clearTimeout(t); state.refreshing.delete(id); hit = true; } } return hit; }
    async function refresh() {
      if (state.refreshing.size || state.reportBusy) return;
      const list = sources(), chosen = list.find(s => s.id === state.selected);
      const bases = [...new Set((chosen ? [chosen] : list).map(s => s.base))];
      state.note = '';
      const answers = await Promise.all(bases.map(id => invoke('refresh_ring', { provider: id }).then(coming => [id, coming === true], () => [id, false])));
      let coming = 0;
      for (const [id, yes] of answers) {
        if (!yes) continue;
        coming++;
        state.refreshing.set(id, setTimeout(() => { state.refreshing.delete(id); schedule(); }, REFRESH_WAIT_MS));
      }
      if (!coming && bases.length) state.note = 'No new reading was requested. These sources are waiting out a rate limit or were read moments ago.';
      if (!chosen || familyOf(chosen.base)) loadReport();
      schedule();
    }

    /* ---- data ---- */
    function take(key, label, value) {
      state.data[key] = value;
      state.failed.delete(label);
      // Event and first-read command names differ; a successful push clears either failure.
      const command = {usage:'get_usage',codex:'get_codex',cursor:'get_cursor',grok:'get_grok',gemini:'get_antigravity',glm:'get_glm',extra:'get_extra_usage',parity:'get_parity_usage'}[key];
      if (command) state.failed.delete(command);
    }
    function wire(event, key, ids) {
      return listen(event, e => {
        state.pushed.add(key);
        take(key, event, e.payload);
        settle(typeof ids === 'function' ? ids(e.payload) : ids);
        schedule();
      }).catch(() => {});
    }
    /* A push that arrived while the first read was in flight is newer, and it stands. */
    function first(command, key) {
      return invoke(command).then(value => { if (!state.pushed.has(key)) take(key, command, value); })
        .catch(e => { if (!state.pushed.has(key)) state.failed.set(command, bounded((e && e.message) || e, 120)); });
    }
    function optional(command, apply, event, fromEvent) {
      let pushed = false;
      if (event) listen(event, e => { pushed = true; (fromEvent || apply)(e.payload); schedule(); }).catch(() => {});
      invoke(command).then(value => { if (!pushed) { apply(value); schedule(); } }).catch(() => {});
    }
    async function start() {
      await Promise.all([
        wire('usage', 'usage', ['claude']), wire('codex', 'codex', ['codex']), wire('cursor', 'cursor', ['cursor']), wire('grok', 'grok', ['grok']),
        wire('antigravity', 'gemini', ['gemini']), wire('glm', 'glm', ['glm']),
        wire('extra_usage', 'extra', Object.keys(EXTRA)), wire('parity_usage', 'parity', payload => Object.keys(payload || {}))
      ]);
      optional('get_glyphs', v => { if (v && typeof v === 'object') state.glyphs = v; }, 'glyphs');
      optional('get_notch_slots', v => { state.slots = Array.isArray(v) ? v : null; }, 'notch_slots');
      optional('get_parity_settings', v => { state.parity = v; }, 'parity_settings');
      optional('get_antigravity_prefs', v => { state.ag = v; }, 'antigravity_prefs');
      optional('get_quota_colors', v => { state.quota = v; }, 'matra-quota-colors');
      optional('get_activity', v => { state.activity = Array.isArray(v) ? v : []; }, 'activity');
      optional('get_state', v => {
        if (!v || typeof v !== 'object') return;
        state.sessions = Array.isArray(v.sessions) ? v.sessions : [];
        if (typeof v.clock_24h === 'boolean') state.clock24 = v.clock_24h;
      }, 'state');
      await Promise.all([first('get_usage', 'usage'), first('get_codex', 'codex'), first('get_cursor', 'cursor'), first('get_grok', 'grok'), first('get_antigravity', 'gemini'), first('get_glm', 'glm'), first('get_extra_usage', 'extra'), first('get_parity_usage', 'parity')]);
      state.loaded = true;
      schedule();
    }
    function outward(command, what) { invoke(command).catch(e => { state.strip = `Could not open ${what}: ${bounded((e && e.message) || e, 160)}`; schedule(); }); }

    /* ---- input ---- */
    function setChart(el) {
      const key = el.dataset.chart, text = el.dataset.readout;
      if (!key) return;
      state.readouts[key] = text;
      const out = doc.querySelector(`[data-readout-for="${key.replace(/["\\]/g, '\\$&')}"]`);
      if (out) out.textContent = text;
    }
    doc.addEventListener('click', e => {
      const el = e.target.closest && e.target.closest('[data-act]');
      if (!el) { const bar = e.target.closest && e.target.closest('[data-readout]'); if (bar) setChart(bar); return; }
      const act = el.dataset.act;
      if (act === 'overview' || act === 'select') {
        state.selected = act === 'select' ? el.dataset.id : null;
        $('dash-scroll').scrollTop = 0;
        const chosen = sources().find(s => s.id === state.selected);
        $('dash-announce').textContent = `Showing ${chosen ? chosen.name : 'Overview'}`;
        // A keyboard press on Open details removes that button, so focus goes to the matching row.
        if (act === 'select' && el.dataset.k && el.dataset.k.startsWith('open:')) {
          render();
          const row = doc.querySelector(`[data-k="src:${el.dataset.id.replace(/["\\]/g, '\\$&')}"]`);
          if (row) row.focus({ preventScroll: true });
          return;
        }
        return schedule();
      }
      if (act === 'refresh') return void refresh();
      if (act === 'reload') return void loadReport();
      if (act === 'activity') return outward('open_activity', 'Project activity');
      if (act === 'settings') return outward('open_settings', 'Settings');
      if (act === 'site') return outward('open_brand_page', 'the website');
      if (act === 'range' && el.dataset.v !== state.range) {
        state.range = el.dataset.v; state.report = null; state.reportError = ''; state.readouts = {};
        return void loadReport();
      }
      if (act === 'measure') { state.measure = el.dataset.v; return schedule(); }
    });
    for (const type of ['mouseover', 'focusin']) doc.addEventListener(type, e => {
      const bar = e.target.closest && e.target.closest('[data-readout]');
      if (bar) setChart(bar);
    });
    doc.addEventListener('keydown', e => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'r') { e.preventDefault(); refresh(); }
    });
    if (root.addEventListener) root.addEventListener('focus', () => {
      if (!state.reportBusy && Date.now() - state.reportAt > REPORT_FOCUS_MS) loadReport();
    });
    setInterval(schedule, 30000); // reset copy and staleness move with time

    render();
    start();
    loadReport();
    return { state, render, refresh };
  }

  const api = { buildSources, orderSources, headlineOf, weeklyOf, staleOf, usedText, resetText, agoText, statusView, sourceNote, glyphHtml, windowHtml, windowsHtml, familyOf, rowsFor, digest, historyDays, overviewHtml, detailHtml, emptyHtml, mount };
  root.MatraDashboard = api;
  if (typeof module !== 'undefined') module.exports = api;
  if (typeof document !== 'undefined' && root.__TAURI__ && document.getElementById('dash')) {
    api.page = mount(document, { invoke: root.__TAURI__.core.invoke, listen: root.__TAURI__.event.listen });
  }
})(typeof window === 'undefined' ? globalThis : window);

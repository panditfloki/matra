/* Settings > Costs: the activity dashboard over get_activity_report. Real local logs only. No sample
   rows exist in this file. A null cost is never drawn as zero: it reads Unavailable, or At least when
   part of the figure could be priced. */
((root) => {
  const inrFormat = new Intl.NumberFormat('en-IN', { style: 'currency', currency: 'INR', maximumFractionDigits: 2 });
  const inr = v => inrFormat.format(v);
  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }
  function count(n) {
    if (!Number.isFinite(n)) return 'Unavailable';
    if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
    if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
    if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
    return String(n);
  }
  const rowTokens = r => (Number(r.input_tokens) || 0) + (Number(r.output_tokens) || 0) + (Number(r.cached_tokens) || 0);
  /* Coverage comes from the report. Without it the same rule is derived, never assumed complete. */
  function coverageOf(amount, tokens, partial, coverage) {
    if (coverage) return coverage;
    if (!tokens) return 'none';
    if (amount === null || amount === undefined) return 'unavailable';
    return partial ? 'at_least' : 'complete';
  }
  function money(amount, coverage) {
    if (coverage === 'none') return '-';
    if (coverage === 'unavailable' || !Number.isFinite(amount)) return 'Unavailable';
    return coverage === 'at_least' ? `At least ${inr(amount)}` : `~${inr(amount)}`;
  }
  function addRow(group, r) {
    group.tokens += rowTokens(r);
    group.turns += Number(r.turns) || 0;
    if (Number.isFinite(r.cost_inr)) { group.priced += r.cost_inr; group.any = true; }
    else {
      group.incomplete = true;
      if (Number.isFinite(r.priced_cost_inr) && r.priced_cost_inr > 0) { group.priced += r.priced_cost_inr; group.any = true; }
    }
    return group;
  }
  const emptyGroup = (id, name) => ({ id, name, tokens: 0, turns: 0, priced: 0, any: false, incomplete: false });
  function groupCost(g) {
    if (!g.tokens) return { text: '-', amount: null };
    if (!g.any) return { text: 'Unavailable', amount: null };
    return { text: g.incomplete ? `At least ${inr(g.priced)}` : `~${inr(g.priced)}`, amount: g.priced };
  }
  function groupBy(rows, id, name) {
    const map = new Map();
    for (const r of rows || []) {
      const key = id(r);
      if (!map.has(key)) map.set(key, emptyGroup(key, name(r)));
      addRow(map.get(key), r);
    }
    return [...map.values()].sort((a, b) => b.tokens - a.tokens || String(a.name).localeCompare(String(b.name)));
  }
  const dayNumber = day => Date.UTC(+day.slice(0, 4), +day.slice(5, 7) - 1, +day.slice(8, 10));
  const dayText = ms => new Date(ms).toISOString().slice(0, 10);
  const shift = (day, by) => dayText(dayNumber(day) + by * 86400000);
  /* One bar for every calendar day of the period, so a day with no activity is visibly a gap. */
  function dayBars(report) {
    const rows = report && Array.isArray(report.rows) ? report.rows : [];
    const groups = new Map(groupBy(rows, r => r.day, r => r.day).map(g => [g.id, g]));
    const seen = [...groups.keys()].sort();
    const from = (report && report.from) || seen[0], to = (report && report.to) || seen[seen.length - 1];
    if (!from || !to) return [];
    const out = [];
    for (let at = dayNumber(from), end = dayNumber(to); at <= end && out.length < 62; at += 86400000) {
      const id = dayText(at);
      out.push(groups.get(id) || emptyGroup(id, id));
    }
    return out;
  }
  function barScale(bars, measure) {
    const value = g => (measure === 'cost' ? (g.any ? g.priced : null) : g.tokens);
    const max = bars.reduce((m, g) => Math.max(m, value(g) || 0), 0);
    return bars.map(g => {
      const v = value(g);
      const unknown = measure === 'cost' && g.tokens > 0 && v === null;
      return { id: g.id, unknown, height: unknown || !v || !max ? 0 : Math.max(3, Math.round(v / max * 100)), group: g };
    });
  }
  function duration(seconds) {
    if (!Number.isFinite(seconds) || seconds < 0) return '';
    if (seconds < 60) return `${Math.round(seconds)}s`;
    const m = Math.round(seconds / 60);
    return m < 60 ? `${m} min` : `${Math.floor(m / 60)}h ${m % 60}m`;
  }
  /* The rate must be a finite, positive estimate. The backend validates again. */
  function parseRate(text) {
    const s = String(text).trim();
    if (!s) return null;
    const n = Number(s);
    return Number.isFinite(n) && n > 0 && n <= 100000 ? n : null;
  }
  function parsePrice(text, required) {
    const s = String(text == null ? '' : text).trim();
    if (!s) return required ? undefined : null;
    const n = Number(s);
    return Number.isFinite(n) && n >= 0 ? n : undefined;
  }
  /* Missing cache prices stay missing: those tokens remain unpriced, not free. */
  function priceEntry(form) {
    const model = String(form.model || '').trim();
    const input = parsePrice(form.input, true), output = parsePrice(form.output, true);
    const read = parsePrice(form.cache_read, false), write = parsePrice(form.cache_write, false);
    if (!model || input === undefined || output === undefined || read === undefined || write === undefined) return null;
    const entry = { model, input, output };
    if (read !== null) entry.cache_read = read;
    if (write !== null) entry.cache_write = write;
    return entry;
  }

  function mount(host, options) {
    const invoke = options.invoke, T = options.ui || (s => s);
    const state = { range: 'week', account: null, project: null, date: null, measure: 'tokens', report: null,
      settings: null, plans: [], error: '', busy: false, token: 0, saving: false, priceNote: '' };
    const say = key => T(key);

    function metric(label, value, hint) {
      return `<div class="act-metric"><span>${esc(say(label))}</span><strong>${esc(say(value))}</strong>${hint ? `<small>${esc(say(hint))}</small>` : ''}</div>`;
    }
    function periodText(r) {
      if (!r || !r.from) return '';
      return r.from === r.to ? r.from : `${r.from} ${say('to')} ${r.to}`;
    }
    function listRows(groups, limit) {
      return groups.slice(0, limit).map(g => `<div class="act-row"><span class="act-name">${esc(g.name)}</span><span class="act-tok">${esc(count(g.tokens))} ${esc(say('tokens'))}</span><span class="act-cost">${esc(say(groupCost(g).text))}</span></div>`).join('');
    }
    function chartHtml(report) {
      const bars = barScale(dayBars(report), state.measure);
      if (!bars.length) return '';
      const cells = bars.map(b => {
        const g = b.group;
        const label = `${b.id}: ${count(g.tokens)} ${say('tokens')}, ${say(groupCost(g).text)}`;
        return `<button type="button" class="act-bar${b.unknown ? ' unknown' : ''}" data-day="${esc(b.id)}" aria-label="${esc(label)}" title="${esc(label)}"><i style="height:${b.unknown ? 4 : b.height}%"></i></button>`;
      }).join('');
      return `<div class="act-chart" style="grid-template-columns:repeat(${bars.length},minmax(0,1fr))">${cells}</div>`
        + `<div class="act-axis"><span>${esc(bars[0].id)}</span><span>${esc(bars[bars.length - 1].id)}</span></div>`
        + `<div class="act-readout" id="act-readout" role="status" aria-live="polite">${esc(say('Hover a bar to inspect that day.'))}</div>`
        + (state.measure === 'cost' && bars.some(b => b.unknown) ? `<div class="item cap act-flat">${esc(say('Short neutral marks mean cost unavailable.'))}</div>` : '');
    }
    function selectHtml(id, label, all, list, value) {
      const items = [`<option value="">${esc(say(all))}</option>`].concat((list || []).map(o => `<option value="${esc(o.id)}"${o.id === value ? ' selected' : ''}>${esc(o.name || o.id)}</option>`));
      return `<label class="act-filter"><span>${esc(say(label))}</span><select id="${id}">${items.join('')}</select></label>`;
    }
    function sessionsHtml(report) {
      const sessions = Array.isArray(report.sessions) ? report.sessions : [];
      if (!sessions.length) return '';
      const rows = sessions.slice(0, 12).map(s => {
        const cost = Number.isFinite(s.cost_inr) ? `~${inr(s.cost_inr)}` : s.tokens ? 'Unavailable' : '-';
        const when = Number.isFinite(s.last_ts) && s.last_ts > 0 ? new Date(s.last_ts * (s.last_ts < 1e11 ? 1000 : 1)).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '';
        const models = Array.isArray(s.models) ? s.models.join(', ') : '';
        return `<div class="act-row act-session"><span class="act-name">${esc(s.project_name || say('Unknown project'))}<small>${esc([when, duration(s.duration_seconds), models].filter(Boolean).join(' · '))}</small></span><span class="act-tok">${esc(count(s.tokens))} ${esc(say('tokens'))}</span><span class="act-cost">${esc(say(cost))}</span></div>`;
      }).join('');
      const more = sessions.length > 12 || report.sessions_truncated;
      return `<div class="sec">${esc(say('Sessions'))}</div><div class="group">${rows}${more ? `<div class="item cap">${esc(say('Showing the most recent 12 sessions in this period.'))}</div>` : ''}</div>`;
    }
    function notesHtml(report) {
      const notes = [];
      if (report.note) notes.push(report.note);
      if (report.partial) notes.push(say('Partial report: some turns could not be priced or some log lines could not be read.'));
      if (Number(report.unpriced_turns) > 0) notes.push(`${report.unpriced_turns} ${say('turns have no known price and add no cost here.')}`);
      if (Array.isArray(report.unpriced_models) && report.unpriced_models.length) notes.push(`${say('Unpriced models:')} ${report.unpriced_models.join(', ')}`);
      if (report.scan && (report.scan.files_skipped || report.scan.lines_skipped)) notes.push(`${say('Skipped while reading logs:')} ${report.scan.files_skipped || 0} ${say('files')}, ${report.scan.lines_skipped || 0} ${say('lines')}`);
      return notes.map(n => `<div class="item cap">${esc(n)}</div>`).join('');
    }
    function reportHtml() {
      if (state.error) return `<div class="group act-gap"><div class="item"><span class="act-error">${esc(state.error)}</span><button class="btn" data-act="reload">${esc(say('Try again'))}</button></div></div>`;
      const report = state.report;
      if (!report) return `<div class="group act-gap"><div class="item cap">${esc(say(state.busy ? 'Reading local logs…' : 'No report loaded yet.'))}</div></div>`;
      const rows = Array.isArray(report.rows) ? report.rows : [];
      const tokens = rows.reduce((sum, r) => sum + rowTokens(r), 0);
      const api = coverageOf(report.total_cost_inr, tokens, report.partial, report.coverage);
      const spend = report.estimated_spend || null;
      const empty = !rows.length;
      let html = `<div class="group act-metrics">${metric('Tokens', empty ? '-' : count(tokens))}`
        + (spend ? metric('Estimated spend', money(spend.amount_inr, spend.coverage), 'Plan share, not a bill') : '')
        + metric('API-equivalent (estimate)', money(report.total_cost_inr, api), 'Comparison at list price') + '</div>';
      if (empty) {
        html += `<div class="group act-gap"><div class="item cap">${esc(say('No Claude or Codex activity was found on this computer for this period.'))}</div>${notesHtml(report)}</div>`;
        return html;
      }
      html += `<div class="group act-gap act-chartwrap"><div class="item"><span>${esc(say('By day'))}</span><span class="seg" id="act-measure"><button data-v="tokens" class="${state.measure === 'tokens' ? 'on' : ''}">${esc(say('Tokens'))}</button><button data-v="cost" class="${state.measure === 'cost' ? 'on' : ''}">${esc(say('Cost'))}</button></span></div>${chartHtml(report)}</div>`;
      const nameOf = r => r.project_name || ((report.projects || []).find(p => p.id === r.project) || {}).name || say('Unknown project');
      html += `<div class="sec">${esc(say('Projects'))}</div><div class="group">${listRows(groupBy(rows, r => r.project, nameOf), 12)}</div>`;
      html += `<div class="sec">${esc(say('Models'))}</div><div class="group">${listRows(groupBy(rows, r => r.model, r => r.model), 12)}</div>`;
      html += sessionsHtml(report);
      html += `<div class="group act-gap">${notesHtml(report)}</div>`;
      return html;
    }
    function accountRows() {
      const report = state.report, s = state.settings;
      const accounts = report && Array.isArray(report.accounts) ? report.accounts : [];
      if (!s || !accounts.length) return '';
      const budgets = report && Array.isArray(report.budgets) ? report.budgets : [];
      return accounts.map(a => {
        const saved = (s.accounts && s.accounts[a.id]) || {};
        const billing = saved.billing === 'api' ? 'api' : 'subscription';
        const budget = budgets.find(b => b.account === a.id);
        const planOptions = [`<option value="">${esc(say('Plan not chosen'))}</option>`].concat(state.plans.map(p => `<option value="${esc(p.tier)}"${p.tier === saved.plan_tier ? ' selected' : ''}>${esc(p.name)}${Number.isFinite(p.inr) ? ` · ${esc(inr(p.inr))}` : ''}</option>`));
        const source = budget && budget.monthly_source;
        const line = budget ? (Number.isFinite(budget.monthly_inr) ? `${inr(budget.monthly_inr)} ${say('per month')}` + (source === 'plan_list_price' ? ` (${say('list price')})` : source === 'you_entered' ? ` (${say('you entered')})` : '') : say('Monthly amount unknown')) : '';
        return `<div class="acct" data-account="${esc(a.id)}"><div class="acct-line"><span class="acct-name"><span>${esc(a.name || a.id)}</span></span><span class="seg" data-billing="${esc(a.id)}"><button data-v="subscription" class="${billing === 'subscription' ? 'on' : ''}">${esc(say('Subscription'))}</button><button data-v="api" class="${billing === 'api' ? 'on' : ''}">${esc(say('API'))}</button></span></div>`
          + (billing === 'subscription' ? `<div class="acct-sub"><span>${esc(say('Plan'))}</span><select data-plan="${esc(a.id)}" aria-label="${esc(say('Plan'))}">${planOptions.join('')}</select></div><div class="acct-sub"><span>${esc(say('You pay per month (INR)'))}</span><input class="act-num" data-paid="${esc(a.id)}" inputmode="decimal" value="${Number.isFinite(saved.monthly_paid_inr) ? esc(saved.monthly_paid_inr) : ''}" placeholder="-" aria-label="${esc(say('You pay per month (INR)'))}"></div>` : '')
          + (line ? `<div class="acct-detail">${esc(line)}</div>` : '') + '</div>';
      }).join('');
    }
    function pricingHtml() {
      const s = state.settings;
      if (!s) return state.priceNote ? `<div class="group act-gap"><div class="item cap" role="status" aria-live="polite">${esc(state.priceNote)}</div></div>` : '';
      const prices = Array.isArray(s.custom_prices) ? s.custom_prices : [];
      const missing = state.report && Array.isArray(state.report.unpriced_models) ? state.report.unpriced_models : [];
      const list = prices.map((p, i) => `<div class="act-row"><span class="act-name">${esc(p.model)}<small>${esc(`$${p.input} ${say('in')} · $${p.output} ${say('out')}${p.cache_read != null ? ` · $${p.cache_read} ${say('cache read')}` : ''}${p.cache_write != null ? ` · $${p.cache_write} ${say('cache write')}` : ''}`)}</small></span><span></span><button class="btn" data-remove-price="${i}">${esc(say('Remove'))}</button></div>`).join('');
      const words = { input: 'Input', output: 'Output', cache_read: 'Cache read', cache_write: 'Cache write' };
      return `<div class="sec">${esc(say('How the money is worked out'))}</div><div class="group">`
        + `<div class="item"><label for="act-rate">${esc(say('USD to INR rate'))}</label><input id="act-rate" class="act-num" inputmode="decimal" value="${esc(s.usd_to_inr)}"></div>`
        + `<div class="item cap">${esc(say('Your estimate, not a live rate. All amounts are estimates in Indian rupees, not bills. Where a price is missing the amount shows Unavailable or At least, never ₹0.'))}</div>`
        + `<div class="item"><span>${esc(say('Per-token prices'))}</span><button class="btn" data-act="refresh-prices">${esc(say('Refresh prices'))}</button></div>`
        + `<div class="item cap" id="act-price-note" role="status" aria-live="polite">${esc(state.priceNote || say('Only fetched when you press Refresh prices. No account data is sent.'))}</div></div>`
        + `<div class="sec">${esc(say('Billing'))}</div><div class="group">${accountRows() || `<div class="item cap">${esc(say('Accounts appear here once a report has loaded.'))}</div>`}</div>`
        + `<div class="sec">${esc(say('Custom prices'))}</div><div class="group">${list}`
        + `<form class="act-price" data-form="price"><input name="model" list="act-unpriced" placeholder="${esc(say('Model'))}" aria-label="${esc(say('Model'))}"><datalist id="act-unpriced">${missing.map(m => `<option value="${esc(m)}"></option>`).join('')}</datalist>`
        + Object.entries(words).map(([k, w]) => `<input name="${k}" inputmode="decimal" placeholder="${esc(say(w))}" aria-label="${esc(say(w))}">`).join('')
        + `<button class="btn" type="submit">${esc(say('Add'))}</button></form><div class="item cap">${esc(say('USD per million tokens. Leave a cache price empty and those tokens stay unpriced.'))}</div></div>`;
    }
    function render() {
      const report = state.report;
      const today = dayText(Date.now() - new Date().getTimezoneOffset() * 60000);
      const canNext = report && report.to && report.to < today;
      const scroll = host.closest ? host.closest('#body') : null;
      const top = scroll ? scroll.scrollTop : 0;
      host.innerHTML = `<p class="matra-intro">${esc(say('What each project spent of each login\'s allowance.'))}</p>`
        + `<div class="group"><div class="item"><span class="seg" id="act-range">${['day', 'week', 'month'].map(r => `<button data-v="${r}" class="${state.range === r ? 'on' : ''}">${esc(say({ day: 'Day', week: 'Week', month: 'Month' }[r]))}</button>`).join('')}</span>`
        + `<span class="act-period"><button class="btn act-step" data-step="-1" aria-label="${esc(say('Previous period'))}"${report && report.from ? '' : ' disabled'}>&lsaquo;</button><span id="act-period">${esc(periodText(report))}</span><button class="btn act-step" data-step="1" aria-label="${esc(say('Next period'))}"${canNext ? '' : ' disabled'}>&rsaquo;</button></span></div>`
        + `<div class="item act-filters">${selectHtml('act-account', 'Login', 'All logins', report && report.accounts, state.account)}${selectHtml('act-project', 'Project', 'All projects', report && report.projects, state.project)}<button class="btn" data-act="reload"${state.busy ? ' disabled' : ''}>${esc(say(state.busy ? 'Reading…' : 'Refresh'))}</button></div></div>`
        + reportHtml() + pricingHtml();
      if (scroll) scroll.scrollTop = top;
    }
    async function load() {
      const token = ++state.token;
      state.busy = true; state.error = ''; render();
      try {
        const report = await invoke('get_activity_report', { range: state.range, account: state.account, project: state.project, date: state.date });
        if (token !== state.token) return;
        state.report = report && typeof report === 'object' ? report : null;
      } catch (e) {
        if (token !== state.token) return;
        state.report = null;
        state.error = `${say('Could not read activity:')} ${String((e && e.message) || e)}`;
      }
      state.busy = false; render();
    }
    async function loadSettings() {
      try {
        const [settings, plans] = await Promise.all([invoke('get_cost_settings'), invoke('get_cost_plans').catch(() => [])]);
        state.settings = settings && typeof settings === 'object' ? settings : null;
        state.plans = Array.isArray(plans) ? plans : [];
      } catch (e) { state.settings = null; state.priceNote = `${say('Could not load cost settings:')} ${String((e && e.message) || e)}`; }
      render();
    }
    async function saveSettings(next) {
      if (state.saving) return false;
      state.saving = true;
      const previous = state.settings;
      try {
        state.settings = await invoke('set_cost_settings', { settings: next });
        state.priceNote = '';
        await load();
        return true;
      } catch (e) {
        state.settings = previous;
        state.priceNote = `${say('Could not save. Your previous cost settings are unchanged:')} ${String((e && e.message) || e)}`;
        render();
        return false;
      } finally { state.saving = false; }
    }
    function accountPatch(id, patch) {
      const accounts = { ...(state.settings.accounts || {}) };
      accounts[id] = { billing: 'subscription', monthly_paid_inr: null, plan_tier: null, ...(accounts[id] || {}), ...patch };
      return { ...state.settings, accounts };
    }
    function readout(id, rows) {
      const out = host.querySelector('#act-readout');
      if (!out) return;
      const g = rows.reduce(addRow, emptyGroup(id, id));
      out.textContent = `${id}: ${count(g.tokens)} ${say('tokens')} · ${say(groupCost(g).text)}`;
    }

    host.addEventListener('click', async e => {
      const el = e.target.closest && e.target.closest('button');
      if (!el) return;
      const seg = el.closest('.seg');
      if (seg && seg.id === 'act-range') { state.range = el.dataset.v; state.date = null; state.project = null; return load(); }
      if (seg && seg.id === 'act-measure') { state.measure = el.dataset.v; return render(); }
      if (seg && seg.dataset.billing) return saveSettings(accountPatch(seg.dataset.billing, { billing: el.dataset.v }));
      if (el.dataset.step && state.report && state.report.from) {
        state.date = Number(el.dataset.step) < 0 ? shift(state.report.from, -1) : shift(state.report.to, 1);
        return load();
      }
      if (el.dataset.act === 'reload') return load();
      if (el.dataset.act === 'refresh-prices') {
        state.priceNote = say('Refreshing prices…'); render();
        try {
          const out = await invoke('refresh_cost_prices');
          state.priceNote = `${say('Prices updated for')} ${Number(out && out.models) || 0} ${say('models.')}`;
          await load();
        } catch (err) { state.priceNote = `${say('Could not refresh prices:')} ${String((err && err.message) || err)}`; render(); }
        return;
      }
      if (el.dataset.removePrice !== undefined) {
        const prices = (state.settings.custom_prices || []).filter((_, i) => i !== Number(el.dataset.removePrice));
        return saveSettings({ ...state.settings, custom_prices: prices });
      }
      if (el.dataset.day) readout(el.dataset.day, (state.report.rows || []).filter(r => r.day === el.dataset.day));
    });
    for (const type of ['mouseover', 'focusin']) host.addEventListener(type, e => {
      const bar = e.target.closest && e.target.closest('[data-day]');
      if (bar && state.report) readout(bar.dataset.day, (state.report.rows || []).filter(r => r.day === bar.dataset.day));
    });
    host.addEventListener('change', e => {
      const t = e.target;
      if (t.id === 'act-account') { state.account = t.value || null; state.project = null; return load(); }
      if (t.id === 'act-project') { state.project = t.value || null; return load(); }
      if (t.id === 'act-rate') {
        const rate = parseRate(t.value);
        if (rate === null) { state.priceNote = say('Enter a rate above 0.'); return render(); }
        return saveSettings({ ...state.settings, usd_to_inr: rate });
      }
      if (t.dataset.plan) return saveSettings(accountPatch(t.dataset.plan, { plan_tier: t.value || null }));
      if (t.dataset.paid) {
        const text = t.value.trim();
        const paid = text === '' ? null : Number(text);
        if (paid !== null && !(Number.isFinite(paid) && paid >= 0)) { state.priceNote = say('Enter what you pay as a number, or leave it empty.'); return render(); }
        return saveSettings(accountPatch(t.dataset.paid, { monthly_paid_inr: paid }));
      }
    });
    host.addEventListener('submit', e => {
      e.preventDefault();
      const f = e.target.elements;
      const entry = priceEntry({ model: f.model.value, input: f.input.value, output: f.output.value, cache_read: f.cache_read.value, cache_write: f.cache_write.value });
      if (!entry) { state.priceNote = say('A custom price needs a model, an input price and an output price.'); return render(); }
      saveSettings({ ...state.settings, custom_prices: (state.settings.custom_prices || []).filter(p => p.model !== entry.model).concat(entry) });
    });
    return {
      show() { loadSettings(); return load(); },
      render,
      state
    };
  }
  root.MatraActivity = { mount, money, coverageOf, groupBy, groupCost, dayBars, barScale, parseRate, priceEntry, count, shift, rowTokens };
  if (typeof module !== 'undefined') module.exports = root.MatraActivity;
})(typeof window === 'undefined' ? globalThis : window);

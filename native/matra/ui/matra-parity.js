/* Mātrā ring rules shared by the notch and Settings. Pure logic with no DOM and no bridge.
   The ordering mirrors macos/Sources/Model/ProviderOrder.swift. Unknown data is never read as 0 or 100. */
((root) => {
  const ORDERS = ['manual', 'remaining', 'burnDeficit'];
  const READINGS = ['usage', 'reset', 'both'];
  const CLICKS = ['menu', 'refresh', 'activity'];
  const ACTIVITY = ['pulse', 'wave', 'none'];
  const defaults = Object.freeze({
    shows_remaining: false, ring_order: 'manual', hide_depleted: false, shut_rings_when_spent: true,
    shows_notch_readings: true, menu_bar_reading: 'both', menu_bar_click_action: 'menu',
    menu_bar_activity_style: 'pulse', shows_limits_in_tray: false, tray_providers: Object.freeze([]),
    weekly_headline:false, claude_daily_pace:false, show_usage_pace:false, asks_provider_on_look:false,
    show_codex_extra_limits:true, weekly_ring_dashed:false, weekly_reading:false, folds_for_fullscreen:true,
    uses_custom_scale:false, custom_scale:1, account_nicknames:Object.freeze({}), account_order:Object.freeze([])
  });
  const HOUR = 3600000, DAY = 24 * HOUR;
  const flag = (v, fallback) => (typeof v === 'boolean' ? v : fallback);
  const pick = (v, list, fallback) => (list.includes(v) ? v : fallback);

  function normalize(value) {
    const v = value && typeof value === 'object' ? value : {};
    return {
      ...v,
      ...Object.fromEntries(['weekly_headline','claude_daily_pace','show_usage_pace','asks_provider_on_look','show_codex_extra_limits','weekly_ring_dashed','weekly_reading','folds_for_fullscreen','uses_custom_scale'].map(k=>[k,flag(v[k],defaults[k])])),
      custom_scale:Number.isFinite(v.custom_scale)?Math.min(1.5,Math.max(.5,v.custom_scale)):1,
      account_nicknames:v.account_nicknames && typeof v.account_nicknames==='object' && !Array.isArray(v.account_nicknames) ? {...v.account_nicknames} : {},
      account_order:Array.isArray(v.account_order)?v.account_order.filter(x=>typeof x==='string'):[],
      shows_remaining: flag(v.shows_remaining, defaults.shows_remaining),
      ring_order: pick(v.ring_order, ORDERS, defaults.ring_order),
      hide_depleted: flag(v.hide_depleted, defaults.hide_depleted),
      shut_rings_when_spent: flag(v.shut_rings_when_spent, defaults.shut_rings_when_spent),
      shows_notch_readings: flag(v.shows_notch_readings, defaults.shows_notch_readings),
      menu_bar_reading: pick(v.menu_bar_reading, READINGS, defaults.menu_bar_reading),
      menu_bar_click_action: pick(v.menu_bar_click_action, CLICKS, defaults.menu_bar_click_action),
      menu_bar_activity_style: pick(v.menu_bar_activity_style, ACTIVITY, defaults.menu_bar_activity_style),
      shows_limits_in_tray: flag(v.shows_limits_in_tray, defaults.shows_limits_in_tray),
      tray_providers: Array.isArray(v.tray_providers) ? v.tray_providers.filter(id => typeof id === 'string') : []
    };
  }
  /* The backend takes the complete object, so a partial change is merged locally first. */
  function merge(saved, patch) { return normalize({ ...normalize(saved), ...patch }); }

  function metered(w) { return !!w && w.count == null && Number.isFinite(w.used) && w.used >= 0; }
  /* What a figure or sweep reads. Colours still judge by what is spent, never by this. */
  function shown(used, showsRemaining) {
    if (!Number.isFinite(used) || used < 0) return null;
    return showsRemaining ? Math.max(0, 1 - used) : used;
  }

  /* Prefer the source duration. Older stored readings may carry an explicit cycle in their
     label or known window ID; other readings have no clock and therefore no pace figure. */
  function durationMs(w) {
    if (!w) return null;
    if (Number.isFinite(w.duration_ms) && w.duration_ms > 0) return w.duration_ms;
    const label = String(w.label || '').toLowerCase();
    const id = String(w.id || '').toLowerCase().split('@')[0];
    const hours = label.match(/(\d+)\s*-?\s*(?:h|hr|hrs|hour|hours)\b/);
    if (hours && Number(hours[1]) > 0) return Number(hours[1]) * HOUR;
    const days = label.match(/(\d+)\s*-?\s*(?:d|day|days)\b/);
    if (days && Number(days[1]) > 0) return Number(days[1]) * DAY;
    if (/five[\s-]hour/.test(label)) return 5 * HOUR;
    if (/week|seven[\s-]day/.test(label)) return 7 * DAY;
    if (/month/.test(label)) return 30 * DAY;
    if (id === 'session' || id === 'five_hour' || id === '5h' || id.endsWith('-5h')) return 5 * HOUR;
    if (id.startsWith('seven_day') || id.startsWith('weekly') || id.endsWith('-weekly')) return 7 * DAY;
    if (id === 'monthly') return 30 * DAY;
    return null;
  }
  /* Remaining quota minus remaining time, in points. Positive means quota will expire unused. */
  function burnPoints(w, now) {
    if (!metered(w) || !Number.isFinite(w.resets_at)) return null;
    const duration = durationMs(w);
    const left = w.resets_at - now;
    if (!duration || !(left > 0)) return null;
    const elapsed = 1 - Math.min(left / duration, 1);
    return (elapsed - Math.min(w.used, 1)) * 100;
  }
  function maxBurn(windows, now) {
    let best = null;
    for (const w of windows || []) {
      const burn = burnPoints(w, now);
      if (burn !== null && (best === null || burn > best)) best = burn;
    }
    return best;
  }

  // Native DailyPace: cumulative sevenths, with days anchored to the vendor's weekly reset.
  // No duration: a pace reading is not itself a cycle against which to measure another pace.
  function dailyPace(weekly, now=Date.now()) {
    if(!metered(weekly)||!Number.isFinite(weekly.resets_at)) return null;
    const start=weekly.resets_at-7*DAY, elapsed=Math.min(7*DAY,Math.max(0,now-start));
    const day=Math.min(6,Math.floor(elapsed/DAY)),allowed=(day+1)/7;
    return {id:'daily_pace',label:'Daily pace',used:weekly.used/allowed,resets_at:day===6?weekly.resets_at:start+(day+1)*DAY,duration_ms:null};
  }
  function pace(w,now=Date.now(),remaining=false) {
    const burn=burnPoints(w,now); if(burn===null) return null;
    const points=-burn, magnitude=Math.abs(points), rounded=Math.round(magnitude*10)/10;
    const value=rounded===0&&magnitude>0?'<0.1':String(rounded);
    const elapsed=1-Math.min((w.resets_at-now)/durationMs(w),1);
    return {points,marker:remaining?1-elapsed:elapsed,text:`${value}% ${points>0?'deficit':'reserved'}`};
  }
  function display(cell,settings,adapter,now=Date.now()) {
    const s=normalize(settings),snap={...cell.snap,windows:[...(cell.snap?.windows||[])]};
    if(cell.base==='codex'&&!s.show_codex_extra_limits) snap.windows=snap.windows.filter(w=>!String(w.id).startsWith('spark')&&!String(w.id).startsWith('code-review'));
    const item={...cell,snap,name:typeof s.account_nicknames[cell.id]==='string'&&s.account_nicknames[cell.id].trim()?s.account_nicknames[cell.id].trim():cell.name};
    let head=adapter.headline(item),second=adapter.weekly(item);
    const length=durationMs(second),headLength=durationMs(head);
    if(s.weekly_headline&&second&&second.id!==head?.id&&length>=6*DAY&&length<=8*DAY&&(!headLength||headLength<length)) [head,second]=[second,head];
    if(s.claude_daily_pace&&cell.base==='claude'&&!snap.windows.some(w=>w.id==='daily_pace')) {
      const weekly=snap.windows.find(w=>['weekly_all','seven_day','weekly'].includes(w.id));
      const daily=dailyPace(weekly,now);
      if(daily){snap.windows.unshift(daily);head=daily;second=snap.windows.find(w=>['session','five_hour'].includes(w.id))||null;}
    }
    snap.presentation_headline=head?.id??null;snap.presentation_secondary=second?.id??null;
    return item;
  }
  function customGlyph(cell,glyphs){
    if(!String(cell.base||cell.id).startsWith('custom:'))return glyphs?.[cell.base||cell.id];
    const detail=cell.snap?.details||{},url=detail.custom_icon_data_url;
    if(typeof url==='string'&&url.length<=350022&&/^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(url))return {kind:'png',url};
    const key=({'openai':'codex','ollama-local':'ollama','gemini-spark':'gemini-api'})[detail.icon_preset]||detail.icon_preset;
    return glyphs?.[key];
  }
  function readingText(head,second,settings,weeklyOn,format) {
    const s=normalize(settings);if(!metered(head))return null;
    const first=`${head.derived?'~':''}${format(shown(head.used,s.shows_remaining))}%`;
    return s.weekly_reading&&weeklyOn&&metered(second)?`${first}/${format(shown(second.used,s.shows_remaining))}%`:first;
  }

  function usedOf(w) { return metered(w) ? w.used : null; }
  /* (tier, remaining): 0 usable, 1 shut with room, 2 fully empty, 3 unmeasurable */
  function remainingKey(cell, a) {
    const used = usedOf(a.headline(cell));
    if (!a.fresh(cell) || used === null) return { tier: 3, burn: null, left: 0 };
    const left = 1 - used;
    if (used >= 1) return { tier: 2, burn: null, left };
    const weekly = usedOf(a.weekly(cell));
    return { tier: weekly !== null && weekly >= 1 ? 1 : 0, burn: null, left };
  }
  /* (tier, burn, remaining): 0 timed usable, 1 timeless usable, 2 shut with room, 3 empty, 4 unmeasurable */
  function deficitKey(cell, a, now) {
    const used = usedOf(a.headline(cell));
    if (!a.fresh(cell) || used === null) return { tier: 4, burn: null, left: 0 };
    const left = 1 - used;
    const burn = maxBurn(cell.snap && cell.snap.windows, now);
    if (used >= 1) return { tier: 3, burn, left };
    const weekly = usedOf(a.weekly(cell));
    if (weekly !== null && weekly >= 1) return { tier: 2, burn, left };
    return burn !== null ? { tier: 0, burn, left } : { tier: 1, burn: null, left };
  }
  function sortStable(cells, key) {
    return cells.map((cell, index) => ({ cell, index, key: key(cell) })).sort((a, b) => {
      if (a.key.tier !== b.key.tier) return a.key.tier - b.key.tier;
      const x = a.key.burn, y = b.key.burn;
      if (x !== y) {
        if (x !== null && y !== null) return y - x;
        if (x !== null) return -1;
        if (y !== null) return 1;
      }
      if (a.key.left !== b.key.left) return b.key.left - a.key.left;
      return a.index - b.index;
    }).map(entry => entry.cell);
  }
  /* A stored order is a preference to reconcile: unseen items follow, unknown ids are skipped. */
  function arrange(items, order, id) {
    if (!Array.isArray(order) || !order.length) return items.slice();
    const rest = items.slice(), out = [];
    for (const want of order) {
      const at = rest.findIndex(item => id(item) === want);
      if (at >= 0) out.push(rest.splice(at, 1)[0]);
    }
    return out.concat(rest);
  }
  function order(cells, mode, a, now = Date.now()) {
    if (mode === 'remaining') return sortStable(cells, cell => remainingKey(cell, a));
    if (mode === 'burnDeficit') return sortStable(cells, cell => deficitKey(cell, a, now));
    return cells.slice();
  }
  /* A window spent or a week spent. An old reading is not proof the account is empty. */
  function isDepleted(cell, a) {
    if (!a.fresh(cell)) return false;
    const used = usedOf(a.headline(cell)), weekly = usedOf(a.weekly(cell));
    return (used !== null && used >= 1) || (weekly !== null && weekly >= 1);
  }
  /* Never returns an empty list: a meter with no rings has nothing left to hover. */
  function present(cells, settings, a, now = Date.now()) {
    const s = normalize(settings);
    const kept = s.hide_depleted ? cells.filter(cell => !isDepleted(cell, a)) : cells;
    return order(kept.length ? kept : cells, s.ring_order, a, now);
  }
  /* While the pointer is on the meter the rings hold still, so the ring under it stays under it. */
  function holdOrder(cells, previousIds, frozen, id = cell => cell.id) {
    return frozen && previousIds && previousIds.length ? arrange(cells, previousIds, id) : cells;
  }
  function isShut(settings, headline, weekly) {
    if (!normalize(settings).shut_rings_when_spent) return false;
    const used = usedOf(headline), week = usedOf(weekly);
    return (used !== null && used >= 1) || (week !== null && week >= 1);
  }
  function shutColor(settings) { return normalize(settings).shows_remaining ? 'var(--matra-muted)' : 'var(--quota-red)'; }

  /* The grip: present whenever the notch is expanded, unless the owner switched it off.
     A press needs no hover and no modifier key, only the left button on the dots. */
  function gripShown(state) { return !state.folded && state.showMove !== false; }
  function gripPress(state) { return state.button === 0 && gripShown(state) && state.onGrip === true; }

  /* An empty list means every provider in its default place; anything else is stored as chosen. */
  function storeSlots(nextIds, allIds) {
    const same = nextIds.length === allIds.length && nextIds.every((id, i) => id === allIds[i]);
    return same ? [] : nextIds.slice();
  }
  /* One place up or down. Returns the same list when it cannot move. */
  function moved(ids, id, step) {
    const at = ids.indexOf(id), to = at + step;
    if (at < 0 || to < 0 || to >= ids.length) return ids.slice();
    const next = ids.slice();
    next.splice(to, 0, next.splice(at, 1)[0]);
    return next;
  }

  root.MatraParity = { defaults, ORDERS, READINGS, CLICKS, ACTIVITY, normalize, merge, metered, shown, durationMs,
    burnPoints, maxBurn, arrange, order, isDepleted, present, holdOrder, isShut, shutColor, gripShown, gripPress,
    storeSlots, moved, dailyPace, pace, display, readingText, customGlyph };
  if (typeof module !== 'undefined') module.exports = root.MatraParity;
})(typeof window === 'undefined' ? globalThis : window);

/* Mātrā ring rules shared by the notch and Settings. Pure logic with no DOM and no bridge.
   The ordering mirrors macos/Sources/Model/ProviderOrder.swift. Unknown data is never read as 0 or 100. */
((root) => {
  const ORDERS = ['manual', 'remaining', 'burnDeficit'];
  const READINGS = ['usage', 'reset', 'both'];
  const CLICKS = ['menu', 'refresh', 'activity'];
  const ACTIVITY = ['pulse', 'wave', 'none'];
  const defaults = Object.freeze({
    shows_remaining: false, ring_order: 'manual', hide_depleted: false, shut_rings_when_spent: false,
    shows_notch_readings: true, menu_bar_reading: 'both', menu_bar_click_action: 'menu',
    menu_bar_activity_style: 'pulse', shows_limits_in_tray: false, tray_providers: Object.freeze([])
  });
  const HOUR = 3600000, DAY = 24 * HOUR;
  const flag = (v, fallback) => (typeof v === 'boolean' ? v : fallback);
  const pick = (v, list, fallback) => (list.includes(v) ? v : fallback);

  function normalize(value) {
    const v = value && typeof value === 'object' ? value : {};
    return {
      ...v,
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

  /* The Windows window type has no duration field, so a clock is only inferred from a name that
     states one. Anything else has no clock, and so no burn figure, rather than a guessed one. */
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
    storeSlots, moved };
  if (typeof module !== 'undefined') module.exports = root.MatraParity;
})(typeof window === 'undefined' ? globalThis : window);

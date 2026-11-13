/* Account identity comes from the profile key, never its display label. */
((root) => {
  function claude(usage) {
    if (!usage || usage.status === 'absent') return [];
    const windows = Array.isArray(usage.windows) ? usage.windows : [];
    if (Array.isArray(usage.accounts) && usage.accounts.length) {
      return usage.accounts.filter(a => a && typeof a.id === 'string' && (a.id === 'claude' || (a.id.startsWith('claude@') && a.id.length > 7))).map(a => ({
        id: a.id, base: 'claude', name: a.id === 'claude' ? 'Claude' : (a.display_name || a.id.slice(7)),
        letter: 'C', glyph: 'C', snap: { ...a, windows: Array.isArray(a.windows) ? a.windows : [] }
      }));
    }
    const accounts = new Map();
    for (const window of windows) {
      const at = String(window.id).indexOf('@');
      const slug = at < 0 ? '' : String(window.id).slice(at + 1);
      if (!accounts.has(slug)) accounts.set(slug, { name: window.group || (slug || 'Claude'), windows: [] });
      accounts.get(slug).windows.push({ ...window, id: at < 0 ? window.id : String(window.id).slice(0, at) });
    }
    if (!accounts.size) accounts.set('', { name: 'Claude', windows: [] });
    return Array.from(accounts, ([slug, account]) => ({
      id: slug ? `claude@${slug}` : 'claude', base: 'claude', name: !slug && accounts.size === 1 ? 'Claude' : account.name, letter: 'C', glyph: 'C',
      snap: { ...usage, windows: account.windows }
    }));
  }
  root.MatraAccounts = { claude };
  if (typeof module !== 'undefined') module.exports = root.MatraAccounts;
})(typeof window === 'undefined' ? globalThis : window);

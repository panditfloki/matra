/* Shared quota semantics. No app accent or provider-brand colours enter this scale. */
((root) => {
  const defaults = Object.freeze({ watch: .5, critical: .7, mode: 'step' });
  const tokens = ['green', 'amber', 'orange', 'red'].map(name => `var(--quota-${name})`);
  function normalize(value = {}) {
    const { watch, critical, mode } = value || {};
    return Number.isFinite(watch) && Number.isFinite(critical) && watch >= .01 && watch <= .88
      && critical >= watch + .009999 && critical <= .89 && ['step', 'ramp'].includes(mode)
      ? { watch, critical, mode } : { ...defaults };
  }
  function tone(usage, settings) {
    if (!Number.isFinite(usage) || usage < 0) return 'var(--matra-muted)';
    const { watch, critical, mode } = normalize(settings);
    if (usage >= .9) return tokens[3];
    if (mode === 'step') return tokens[usage >= critical ? 2 : usage >= watch ? 1 : 0];
    const stops = [0, watch, critical, .9];
    const index = usage >= critical ? 2 : usage >= watch ? 1 : 0;
    const mix = Math.max(0, Math.min(100, (usage - stops[index]) / (stops[index + 1] - stops[index]) * 100));
    if (mix === 0) return tokens[index];
    return `color-mix(in oklab, ${tokens[index]} ${(100 - mix).toFixed(3)}%, ${tokens[index + 1]})`;
  }
  root.MatraQuota = { defaults, normalize, tone };
  if (typeof module !== 'undefined') module.exports = root.MatraQuota;
})(typeof window === 'undefined' ? globalThis : window);

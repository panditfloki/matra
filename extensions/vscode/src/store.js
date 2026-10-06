'use strict';

const { IDS, NAMES, meta } = require('./registry');

class UsageStore {
  constructor(providers, { now = Date.now, timeoutMs = 30000 } = {}) {
    this.providers = providers;
    this.now = now;
    this.timeoutMs = timeoutMs;
    this.listeners = new Set();
    this.disposed = false;
    this.inFlight = null;
    this.snapshot = { schemaVersion: 1, generatedAt: now(), providers: providers.map(p => ({ id: p.id, name: meta(p.id).name, status: 'unavailable', message: 'Reading local usage…', windows: [], updatedAt: null })) };
  }
  subscribe(fn) { this.listeners.add(fn); return { dispose: () => this.listeners.delete(fn) }; }
  emit() { for (const fn of this.listeners) fn(this.snapshot); }
  refresh({ force = false } = {}) {
    if (this.disposed) return Promise.resolve(this.snapshot);
    if (this.inFlight) return this.inFlight;
    this.inFlight = (async () => {
      const records = await Promise.all(this.providers.map(async provider => {
        let timer;
        try {
          let record = await Promise.race([
            Promise.resolve().then(() => provider.read({ now: this.now(), force })),
            new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('timeout')), this.timeoutMs); })
          ]);
          if (!record || record.id !== provider.id || !Array.isArray(record.windows)) throw new Error('Invalid provider result');
          record = { ...record, monogram: meta(provider.id).monogram };
          if (record.status === 'ready' && record.windows.some(w => typeof w?.resetsAt === 'number' && Number.isFinite(w.resetsAt) && w.resetsAt <= this.now())) {
            return { ...record, status: 'stale', message: `${record.message ? `${record.message} ` : ''}A recorded reset has passed. Waiting for the provider to report the new window.` };
          }
          return record;
        } catch {
          // Never display raw provider errors, which can contain account paths or secrets.
          return { id: provider.id, name: meta(provider.id).name, status: 'error', message: 'Could not read this provider. Refresh to try again.', updatedAt: null, windows: [], sessions: [] };
        } finally { clearTimeout(timer); }
      }));
      if (!this.disposed) {
        this.snapshot = { schemaVersion: 1, generatedAt: this.now(), providers: records };
        this.emit();
      }
      return this.snapshot;
    })().finally(() => { this.inFlight = null; });
    return this.inFlight;
  }
  dispose() { this.disposed = true; this.listeners.clear(); for (const p of this.providers) p.dispose?.(); }
}

module.exports = { UsageStore, IDS, NAMES };

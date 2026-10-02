// Evidence-based visual events, independent from layout and provider fetches.
(() => {
  class MotionTracker {
    constructor(){ this.readings = new Map(); this.sessions = new Map(); this.initialized = false; this.doneEvents = new Set(); this.activityInitialized = false; }
    activity(events){
      const completed = [];
      for(const a of events || []){
        if(a.state !== 'done' || !Number.isFinite(a.since)) continue;
        const key = a.provider + ':' + a.since;
        if(this.activityInitialized && !this.doneEvents.has(key)) completed.push(a.provider);
        this.doneEvents.add(key);
      }
      this.activityInitialized = true;
      if(this.doneEvents.size > 200) this.doneEvents = new Set(Array.from(this.doneEvents).slice(-100));
      return Array.from(new Set(completed));
    }
    reading(id, windowId, used, fresh){
      const key = id + ':' + windowId, previous = this.readings.get(key);
      if(!fresh || !Number.isFinite(used) || used < 0){ this.readings.delete(key); return null; }
      this.readings.set(key, used);
      return previous != null && Math.abs(used-previous) > 0.00001 ? {from:previous,to:used} : null;
    }
    claude(sessions){
      let completed = false; const next = new Map();
      for(const s of sessions || []){
        const before = this.sessions.get(s.id);
        if(this.initialized && before && ['running','attention'].includes(before.state) && s.state === 'done') completed = true;
        next.set(s.id, {state:s.state,started:s.started});
      }
      this.sessions = next; this.initialized = true;
      return completed; // Startup, disappearance and idle timeouts are not completion.
    }
  }
  window.MatraMotionTracker = MotionTracker;
})();

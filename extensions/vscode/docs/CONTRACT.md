# Standalone extension contract

The extension runs without the desktop app or local web server. Use Node built-ins, stable VS Code APIs and CommonJS. No runtime package download, no arbitrary shell execution, no telemetry. The extension host is local (`extensionKind: ["ui"]`). Keep credentials and transcript contents out of logs and webviews. Provider data is read-only; only extension-owned cache files may be written. Respect provider rate limits. Never modify provider credentials or refresh tokens.

## Provider interface

Each module exports `createProvider(options)` returning `{ id, read({ now = Date.now(), force = false } = {}) }`. `read` is async and returns a provider record, including unavailable/auth/error states rather than rejecting for expected failures. `options` contains `storagePath` (extension-owned absolute cache folder), `homeDir` (defaults to os.homedir()), and optional `fetch`, `execFile`, `platform` injection for tests. Optional explicit data paths come from `options.paths` only. Do not depend on VS Code in these modules. Use self-contained helper modules with provider-specific filenames to avoid writer overlap.

Provider record:

```js
{
  id: 'claude', // claude | codex | cursor | antigravity
  name: 'Claude Code',
  status: 'ready', // ready | stale | unavailable | needs-auth | error
  message: '', // sanitized actionable explanation
  source: 'Claude Code',
  updatedAt: 1791158400000, // milliseconds or null; actual observation time
  account: { label: null, plan: 'Max' }, // optional; mask identity, never tokens
  headlineId: 'session',
  windows: [{ id: 'session', label: 'Current session', usedPercent: 12, resetsAt: 1791176400000 }],
  usage: { period: 'Today', inputTokens: 10, outputTokens: 20, cachedTokens: 0, totalTokens: 30, costUsd: null }, // optional; unknown is null, cost must be labelled estimate if calculated
  sessions: [{ id: 'opaque-local-id', label: 'Project name', detail: 'Model', lastActiveAt: 1791158400000, state: 'recent', tokens: 30 }] // optional, <= 20, no prompt text or full paths
}
```

Percentages are finite real observations, can exceed 100. Missing quota is null or no windows, never 0. Dates use epoch milliseconds; expired resets never imply zero usage. Keep each quota bucket separate. Preserve cached observations on transient failures with stale status and original updatedAt, but never retain another account's data after account change/sign-out. Unknown cost is null. Session recency is not evidence of a running process; use `recent` / `idle` / `unknown` unless activity is proven.

The aggregate is `{ schemaVersion: 1, generatedAt: Date.now(), providers: [...] }`.

## UI and acceptance

Activity Bar sidebar and full dashboard, status-bar summary, refresh, provider visibility and persisted reset mode (`date` / `remaining`). Exact date is localized date + time + year across year boundaries. Countdown shows days/hours or hours/minutes and says `Resetting…` after expiry. Theme tokens, keyboard controls, no animation dependency, narrow sidebar support. Webviews use nonce CSP and escaped data. Only allowlisted messages/URLs may cause host actions.

One local VSIX with explicit package allowlist, no private files. Automated provider/formatter/host lifecycle tests, actual Extension Development Host test, sample rendered UI, package contents audit. Distinguish source/build verification from installed IDE/live-account acceptance. No marketplace publication in this task.

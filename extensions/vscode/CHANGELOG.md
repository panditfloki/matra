# Changelog

## 0.1.3

- Codex quota shows again. Codex 0.160+ tags its logs with `creator_account_id`; the reader only looked for `account_id`, so every log written before the last token refresh was dropped and the card said "Not available".
- The "earlier logs cannot be attributed" warning now appears only when a log was actually excluded.

## 0.1.1

- Status bar and hover card restored to the v1.7 look: coloured bars of quota left, per-window text meters, plan and account, reset countdowns and today's tokens.
- New `matra.statusBarBars` setting to turn the coloured bars off.

## 0.1.0

- Standalone provider readers for Claude Code, Codex, Cursor and Antigravity.
- Activity Bar sidebar, dashboard and status-bar summary.
- Persisted reset date/time or countdown display.
- Honest missing, stale and authentication states, with local account isolation.
- Restricted webview messages, nonce CSP and a package allowlist.

# Mātrā for VS Code

Your AI usage, inside your editor. A standalone extension for Claude Code, Codex, Cursor and Antigravity.

Mātrā's desktop app and local web server are not required. The extension reads the accounts and usage records belonging to your installed tools. No telemetry, no transcript upload and no runtime package downloads.

## Install the local preview

1. Open the editor's Extensions view.
2. Choose **Install from VSIX…** from its menu and select `matra-0.1.0.vsix`.
3. Open the Mātrā Activity Bar icon, or run **Mātrā: Open Dashboard**.

The extension uses the stable VS Code API, version 1.85 or newer, in the local desktop extension host. VS Code forks require their own installation checks. Browser-only editors are not supported. Remote SSH windows show the accounts on your local machine, not on the remote server.

## What you can see

- Separate provider usage windows, remaining percentages and actual observation age.
- **Date & time**: the actual reset date and local clock time.
- **Time left**: days, hours and minutes remaining. Expired windows say **Resetting…** until a new reading arrives.
- A compact status-bar summary. A `~` marks a stale observation.
- Token usage and recent sessions where local records supply them. Costs appear only where known and are labelled estimates.

Use **Manage providers** to hide tools you do not use. Disabled providers are not polled. Settings apply to your user profile and cannot be supplied by a workspace repository.

## Provider setup

| Provider | Data source and requirements |
|---|---|
| Claude Code | Existing Claude Code sign-in. Reads its OAuth credential and quota endpoint, plus local usage logs. macOS may request Keychain access. |
| Codex | Existing Codex sign-in and local session records. Quota is the most recent account-matched observation recorded by Codex, rather than a fresh account API request. Run Codex to generate a newer reading. |
| Cursor | Existing Cursor editor or supported CLI sign-in. Reads its account store and usage-summary endpoint. SQLite access uses the extension host's built-in SQLite when available, then a local SQLite/Python helper. |
| Antigravity | A running Antigravity IDE/CLI language server. Reads that provider's authenticated loopback RPC. Closing Mātrā desktop has no effect; closing Antigravity's language server removes this data source. |

Mātrā does not sign you in, rotate tokens or change another tool's credentials. Missing sign-in, missing tools, API rate limits and stale observations have distinct states. Unknown usage is never reported as zero. Account changes clear the old account's readings.

Recent session entries show local recency, not proof that a process is running. Bounded scans can provide partial token totals; see each provider's message. No subscription cost is inferred from a token count.

## Commands and settings

Commands: **Open Dashboard**, **Refresh Usage**, **Choose Reset Display**, **Choose Providers**, **Open Settings**, all prefixed with **Mātrā**.

Settings: `matra.resetDisplay`, `matra.providers`, `matra.statusBar`, `matra.refreshInterval`. Optional absolute roots are `matra.paths.claude` and `matra.paths.codex`. Provider cooldowns apply even when you request a refresh. All dates use your machine's local time zone.

## Develop

```sh
npm test
npm run check
vsce package --no-dependencies
```

There are no runtime npm dependencies and no compilation step. Tests use Node's built-in runner. Packaging uses Microsoft's `@vscode/vsce` development tool. `test/host/index.js` is an Extension Development Host acceptance runner and is excluded from the VSIX. The package allowlist includes only metadata, `src/` and `media/`.

This is a local preview. Desktop-companion mode and marketplace publication are separate follow-up work. Microsoft Marketplace and Open VSX require separate publication and publisher setup.

© 2026 dydxfx · https://dydxfx.com

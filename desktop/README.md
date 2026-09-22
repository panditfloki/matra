# Mātrā Desktop 2.2 preview

Windows-first desktop shell in the existing repository. The provider adapters and
dashboard remain shared with the web app and IDE extension. The extension's 1.7.5
manifest version is independent of the desktop preview version in version.json.

## Run and build

```
npm ci
npm run desktop
npm run desktop:build
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/install-desktop.ps1
```

The Windows build copies Electron plus an explicit application allowlist. It bundles
the build machine's ccusage native executable and official SQLite executable, so the
installed app needs neither a terminal nor a global Node installation. The build
currently requires those tools to be installed in their normal npm/WinGet locations.
No credentials, plan.json, agent instructions or local logs enter the package.
Third-party redistribution notices still require review before public distribution.

## Included

- Compact provider rings, hover/focus detail, remaining-capacity semantics.
- Separate quota/history states, stale reasons and source timestamps.
- All four screen edges, monitor cycling, pin-open, tray and start-at-login setting.
- Existing detailed dashboard inside a desktop window.
- Single instance, isolated renderer/preload bridge, background provider process.
- Reuse of the local server when present; independent adapters when absent.
- Helpers resolved from the packaged tools directory before inherited PATH.

First launch opens pinned so the app is visible. Press Pin to release it. Hover the
provider cells for detail; Settings selects edge and monitor. Tray menu opens the
dashboard, refreshes, restarts the provider worker or quits.

## Validation

```
npm test
npm run desktop:smoke
```

For the actual packaged executable use --smoke-test. Set
MATRA_DESKTOP_STANDALONE=1 to bypass the existing server and
MATRA_DESKTOP_TEST_OUT to a writable diagnostic directory. The test captures real
rendered pages, exercises provider tabs, opens the dashboard and records placements.
Diagnostic outputs contain local usage data and must not be published.

## Remaining work before a stable 2.2 release

Windows Gemini live-quota discovery, observed agent activity/alerts, account-profile
management, provider reordering, auto-hide on fullscreen, signed installer/updater,
cross-process refresh coordination when desktop owns polling, bundled-tool license
audit, and native Mac packaging/testing. The current installer is per-user and
unsigned; it does not replace the existing extension or server.

The small widget is new Mātrā UI inspired by CodeNotch's interaction pattern. No
CodeNotch source or assets are included.

# Independent review: dy/dx.f(Mātrā) 2.2.0-preview.11 (Claude, 2026-09-22)

Scope: the Rust/Tauri app under `native/` and its installer, as built and installed on the ROG
(installed exe md5 `d355fea9…` = `native/target/release/codenotch.exe`). Read-only: no app code
edited, nothing installed, no credentials touched, no processes stopped. The running instance
(PID 20456) was observed, not driven. Evidence classes are labelled throughout:
**RUNTIME** = observed on the live machine · **TEST** = an automated suite executed here ·
**CODE** = inspection only · **INFERRED** = arithmetic from code, not observed.

Delta measured against the pinned upstream (`117a38b8`, checked out 13:48 today):
main.rs +20/-0 · config.rs +24/-2 · notch.html +61/-27 · settings.html +11/-3 · tray/autostart/
settings_window cosmetic · plus `ui/matra-theme.css` (109) and `ui/matra-theme.js` (52).
Everything else, including every provider adapter, hit-testing, the local server and the
updater, is upstream code. DESIGN.md's preview.9/10/11 behaviours all live in those ~250 lines.

## Findings, ranked

### P1-1 [install] Upgrading leaves the previous version running, and the failure is masked by a Settings window
- `scripts/install-native.ps1:13` stops only processes whose `Path -EQ` the **new** version directory. A running older preview (any of the nine directories under `%LOCALAPPDATA%\Programs\MatraNotch\`) is never stopped.
- `native/codenotch/src/main.rs:1698-1703`: `tauri_plugin_single_instance` is keyed on the identifier `com.dydxfx.matra.notch`, shared by every version. The installer's `Start-Process` at line 47 is therefore refused, the callback logs `single instance: another launch was refused` and **opens the old version's Settings**, which is exactly what a successful "launch again for Settings" looks like.
- Evidence: CODE for the mechanism; RUNTIME that nine preview directories exist here, so the upgrade path is exercised routinely; whether it misfired before is unrecorded because `run.log` is truncated per launch (first line of the current log is a startup line).
- Impact: shortcuts and the Run key point at N while N-1 keeps serving the notch until the next login. Every "installed preview.N" claim made from this script is unverified by the script itself.
- Fix: match `Path -like "$env:LOCALAPPDATA\Programs\MatraNotch\*\codenotch.exe"` (never by process name, so the original CodeNotch and the MATRA-V2 build are untouched); after copying, wait until port 48676 is free; after launch, read the new PID's `ExecutablePath` and **fail the script** if it is not the new directory.

### P1-2 [data] Mātrā receives zero Claude Code hook events on this machine; the hook identity collides with the original CodeNotch
- RUNTIME: `~/.claude/settings.json` wires all seven events to `D:\…\MATRA-V2\upstream-codenotch\windows\target\release\codenotch-hook.exe` (the 2026-09-09 upstream build, port 48666). Mātrā's `run.log` contains 0 hook-sourced lines.
- CODE: `native/codenotch-hook/src/main.rs:9` reads port 48676 from `matra-notch\config.json` (correctly separated), but the binary is still named `codenotch-hook.exe`, and `hooks_install.rs:29` identifies "ours" by the substring `codenotch-hook`. Either product's Install evicts the other's entries; either product's Uninstall removes both.
- Impact: the Claude cell's activity state (running / attention / done) never fires in Mātrā here, and the tray "install hooks" action would silently break the other app.
- Fix: rename to `matra-hook.exe`, match on `matra-hook` only, treat foreign `codenotch-hook` entries as not ours, and have Doctor/Settings print which path currently owns each event.

### P2-3 [auth/network] Live Codex and Cursor reads fail under TLS inspection because the HTTP client is rustls, not the OS store, despite Cargo.toml saying otherwise
- RUNTIME: 12 × `codex: live read failed (https://chatgpt.com/backend-api/wham/usage: … invalid peer certificate: UnknownIssuer)` and 12 × the same for `cursor.com`. A direct handshake from this machine shows `chatgpt.com` presented by **Kaspersky Anti-Virus Personal Root Certificate**, i.e. TLS interception is active here.
- CODE: `usage.rs:472`, `codex.rs:194`, `cursor.rs:206`, `grok.rs:252` use `ureq::get` / a default `AgentBuilder` (rustls, bundled roots). Only the Antigravity local bridge builds a `native_tls` connector (`antigravity.rs:245-250`). `Cargo.toml:25` enables the `native-tls` feature and comments as if it were in use; it is not, for these calls.
- Mitigation already present, verified in code: the Codex fallback is honest. `codex.rs:648,668-678` sets `note = "Live read failed (…) · <plan> · from last Codex run"`, uses the rollout's recorded time as `fetched_at`, and marks `stale` after 5 min (`CURRENT_FOR_MS`, line 41). `codex.json` on disk shows a later live success (`Prolite · via Codex`), so the failure is intermittent here.
- Impact: P2 on this machine (labelled fallback), P1 on any corporate laptop with permanent inspection (the Telstra device), where live quota will never load.
- Fix: one shared `AgentBuilder` with `.tls_connector(native_tls)` (schannel) for every remote provider, and log the certificate issuer on `UnknownIssuer`.

### P2-4 [hit-testing] Invisible satellites and the bounding-box rule stop click-through over transparent screen
- CODE: `notch.html` `reportHot` (delta @1231) pushes the orb and move rectangles whenever `dockActive` is true, regardless of `hovered`, while CSS keeps them at `opacity:0; pointer-events:none` until hover. `main.rs:927-935` `cursor_in_hot` then treats the **bounding box of all rectangles** as inside.
- RUNTIME: the log shows exactly this state: `click-through on … rects=[pill, tail, card, [11.98,397.75,25.6,25.6], [11.98,96.64,25.6,25.6]]`, the last two being satellites 300 px apart while a card is open. The union covers the whole column.
- Impact: clicks on visibly empty desktop in that column reach nothing (the div under them is `pointer-events:none`). Sharper than REVIEW #3, which only names the corners.
- Fix: report a satellite rectangle only while `hovered` is that satellite or the pointer is on its 10 px bridge; replace the bounding box with explicit bridge rectangles (pill↔card, pill↔each satellite).

### P2-5 [install] No uninstall, no rollback, dead NSIS config
- RUNTIME: the installer leaves a Run key (`MatraNotch … --silent`), two Start Menu shortcuts (`Matra Desktop.lnk`, `dy-dx.f(Mātrā).lnk`), nine version directories, `%APPDATA%\matra-notch`, and WebView2 data at `%LOCALAPPDATA%\com.dydxfx.matra.notch`. No script or menu removes any of it; no Add/Remove entry exists. `tauri.conf.json` declares an `nsis` bundle target but nothing in the repo runs the bundler (grep across scripts/package.json: none), so the only install path is the copy script.
- Rollback today = hand-edit two shortcuts and the Run key to an older directory.
- Fix: `uninstall-native.ps1` (Run key, shortcuts, optional data), a `-Rollback <version>` switch, and pruning to N-1 directories.

### P2-6 [layout] Fixed 650 px long side has no account-overflow strategy (REVIEW #2 confirmed by reading)
- CODE: `main.rs:45` `NOTCH_LONG = 650`; `claudeCells()` (`notch.html:916-933`) emits one cell per `group`; the pill is `overflow:hidden`. INFERRED: at Large the page zoom is 1.25 (`fitZoom`, `notch.html:1260`), so the usable length is ~520 CSS px and the clip arrives at fewer cells than REVIEW's scale-1 arithmetic. Untested: this machine has one Claude account.
- Fix: as REVIEW (bounded selection or overflow menu), or grow the long side with cell count up to the work area.

### P2-7 [a11y] Notch controls are mouse-only (REVIEW #4 confirmed)
- CODE: `notch.html:232,243` are bare `<div>`s, no role/name/tab stop; the window is `focusable:false` by design (`tauri.conf.json`), so no keyboard path exists to the notch at all. Settings is keyboard-operable (13 role/tabindex/keydown sites) and reachable by relaunching the exe or from the tray.
- Fix: keep the notch non-focusable; expose "move to edge" and "open settings" as tray items and a documented hotkey; mark satellites `aria-hidden`.

### P3-8 [theme] Two sources of truth for the theme, and a text-rewriting MutationObserver
- CODE: `matra-theme.js:4` paints from `localStorage` first, then `get_matra_theme`; a theme changed elsewhere flashes the stale one for a frame. `brand()` walks every added node on every ring re-render and rewrites any text containing "Codenotch", including user data. Fix: paint after `invoke`, and replace the strings in the HTML once.

### P3-9 [install] `--silent` is passed by the Run key and handled nowhere (`main.rs`: 0 hits). Harmless, misleading.

### P3-10 [docs] DESIGN.md omits that the move-handle glyph changed (hand → six-dot grip, notch.html diff @237); `settings.html:365,373` captions still read "Codenotch …" and depend on the runtime rewrite.

### P3-11 [security, for release not preview] Updater plugin, `updater:default` capability and three commands stay compiled in with a placeholder key; `updater.rs:64-72` makes every call a no-op (fails closed, verified). `csp: null` is inherited from upstream; acceptable while all rendered provider text passes `esc()` (`notch.html:1205`, the only raw sinks are template literals at 1015/1168), but set a CSP before any public build.

## Security review (item 7), summary
- Local server: binds `127.0.0.1` only (`server.rs:11`), POST-only, Origin / `Sec-Fetch-Site` guard with unit + integration tests (TEST: passed). Any local process can still POST events; the body is rendered through `esc()`, so the exposure is state noise, not injection.
- Credentials: read-only from `~/.claude/.credentials.json` and `~/.codex/auth.json`; expired tokens are never sent (`usage.rs:13`); 401 → one re-read then `needsAuth`; 429 → exponential backoff with persisted deadline. RUNTIME: no token appears in `run.log`, `usage.json`, `codex.json` or `antigravity.json` (masked scan). Sign-in spawns the CLI in its own PowerShell with the path passed by env, not interpolated, and Claude env scrubbed (`claude_auth.rs:37-59`, TEST: passed).
- External links: fixed table only (`main.rs:760-767`, `settings_window.rs:79`).
- Tauri surface: `core:default` + drag/close + updater for notch/settings; dropzones get listen/unlisten only.
- Dependencies: `tauri-plugin-updater 2.11.0` published 2026-08-31 (22 days, gate passes); `tauri 2.11.5`, `tiny_http 0.12.0`, `ureq 2.12.1`. `scripts/audit-native-dependencies.js` exists but has never written its output (`.desktop-test/native-dependency-audit.json` absent).

## Performance (item 8), RUNTIME
PID 20456: ~0.05% CPU over 5 s with the 50 ms watchdog running, 61 threads, 426 handles, 91 MB working set, 7 WebView2 helper processes. No panics or `error` lines in a 1,634-line log.

## Test quality (item 9)
- `cargo test --release --locked`: **132 passed, 0 failed, 3 ignored** (TEST). Covers config round-trip incl. the new `theme` field, work-area/taskbar geometry, origin guard, Codex parsing, auth gate.
- `native/scripts/check-ui-scripts.mjs`: 3 pages parse (TEST). `test-claude-auth-ui.cjs`: PASS. `test-ko-i18n.cjs`: 2/2 (TEST).
- `scripts/native-{handle,layout,ring,theme,compact}-test.js`: **not run**. They require the app launched with WebView2 remote debugging on 9337, which is not listening for the installed instance; relaunching is disruptive and was not done. By construction they drive page state directly (`setDockActive(true)`, `setHovered('move')`, `tone()` tables) and read computed CSS, so they prove the CSS/geometry contract and threshold tables, not pointer behaviour, hover races or click-through. REVIEW's own statement of this is accurate.
- `native-layout-test.js` still asserts the pre-preview.11 visibility contract (both handles visible on body hover) and is not a valid gate for this build.
- `test/*.test.js` under `npm test` cover the Node meter and the retired Electron model, not `native/`.
- Missing coverage: install/upgrade/rollback, single-instance across versions, hook wiring ownership, TLS under interception, multi-account overflow, sleep/resume, taskbar relocation, mixed DPI.

## Matrix

| Area | Tested | Failed | Blocked | Untested |
|---|---|---|---|---|
| 1 Install / upgrade / single-instance / uninstall | binary parity (RUNTIME), single-instance mechanism (CODE) | P1-1, P2-5 | live upgrade drill (would stop his running instance) | rollback |
| 2 Folding, four edges, sizes, mixed DPI, taskbar, sleep, overflow | fold/unfold + edge=left + scale .8 observed in log; taskbar/work-area logic covered by cargo tests | | edge/size sweep (needs 9337) | mixed DPI (both monitors 100%), sleep/resume, taskbar move, >1 account |
| 3 Hover controls, gaps, animation, click-through, gear | gear SVG unchanged (CODE); 2 px gap and 160/180 ms in CSS (CODE) | P2-4 | pointer travel (needs 9337 + physical mouse) | |
| 4 Themes / contrast | palette matches DESIGN tokens (CODE); contrast by arithmetic: dark text 17:1, light 16:1, muted ≥7:1; glass ≥7:1 over a black desktop (INFERRED) | | on-screen measurement | |
| 5 Rings / thresholds / labels / reset / stale / count-only | thresholds 50/75/90 and non-metered dashed track (CODE, matches `native-ring-test` table); reset formatting locale-aware (CODE); stale = 15 min (`notch.html:995`) | | | live rendering of each state |
| 6 Auth / expiry / rate limit / offline / isolation | code paths for 401/429/expired (CODE); live Claude ok (RUNTIME) | P2-3 | expired-credential drill (would alter his credentials) | offline/reconnect |
| 7 Security | server guard tests (TEST), sinks, links, capabilities, secrets in logs (RUNTIME) | | | CSP absence at release |
| 8 Performance / cleanup / a11y / errors | CPU/RAM/threads (RUNTIME) | P2-7 | | leak over days |
| 9 Test quality | cargo, parser, two vm tests (TEST) | layout test obsolete | five CDP tests (9337) | |

## Reassessment of REVIEW-2026-09-22.md
- #1 P1 old version survives: **confirmed by reading both sides**, and sharpened: the refused launch opens the old version's Settings, so the failure looks like success.
- #2 P1 overflow: **confirmed by reading**; worse at Large (zoom 1.25). Not reproduced (one account here).
- #3 P2 hit region: **confirmed and extended**: the satellites are reported while invisible, not only the corners. Runtime log proves the five-rect state.
- #4 P2 mouse-only: **confirmed**; keyboard access is structurally absent by `focusable:false`, so the fix belongs in the tray, not the notch.
- Known gaps: Glass = translucency, updater disabled, Mac parity, shared runtime: all **accurate**. Add two gaps the review did not list: hook identity collision (P1-2) and rustls under TLS inspection (P2-3).
- Verification limits section: **accurate**, including its own admission that the CDP tests are synthetic.

## Top five fixes, in order
1. Installer: stop any `MatraNotch\*\codenotch.exe`, wait for the port, verify the new PID's path, fail loudly (P1-1).
2. Rename the hook to `matra-hook.exe`, match only on that, show hook ownership in Doctor (P1-2).
3. One `native_tls` agent for every remote provider; log the issuer on failure (P2-3).
4. Report satellite rectangles only while hovered/bridged; explicit bridges instead of the bounding box (P2-4).
5. `uninstall-native.ps1` + rollback switch + directory pruning (P2-5).

## Verdict
**Not release-ready; acceptable as a personal preview on this machine.** Nothing here corrupts data or leaks a credential. What blocks release is that the two operational guarantees a user relies on, "the version I installed is the one running" and "the Claude activity ring reflects my session", are both false on the review machine, and one of them is masked by design. Limitations of this review: no physical pointer, sleep/resume, taskbar, mixed-DPI or multi-account runs; the five CDP suites were not executed; the upgrade defect was proven by code, not by a drill, because a drill would have stopped his running instance.

Out of scope, noted once: Kaspersky is active on the ROG again (its root signed `chatgpt.com` in the probe). The vault records it as removed after the 2026-07-17 freeze diagnosis.

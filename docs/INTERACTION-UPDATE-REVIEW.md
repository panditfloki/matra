# Local interaction and updater review

Status: v1.8.2 source prepared for the approved GitHub push. No new public release
or installed-app replacement.
The installed/public v1.8.1 still has the old, disabled updater. This code must
first be installed in a reviewed later build before in-app updates are available.

## Ring behaviour

- Existing inner working spinner remains separate from quota percentage.
- Explicit Claude session `done` and Codex rollout `task_complete` trigger a single
  one-second sweep. Aborts, vanished sessions and silent timeouts do not.
- Completion requests a provider refresh, at most once per 30 seconds, retaining
  the existing provider rate-limit/backoff checks. The percentage is never guessed.
- All providers animate a fresh changed quota reading over 650 ms. Missing, stale,
  unchanged and first-load readings do not pretend to consume tokens.
- Activity starts, completion and changed readings can briefly open hover-mode
  notch. The normal fold logic resumes after roughly two seconds. Pointer, drag,
  carry and menu interactions prevent collapse. Always-visible mode stays visible.
- Reduced-motion preference replaces the spinning feedback with a static highlight.
- Cursor, Antigravity, Grok and GLM do not gain a fabricated per-turn completion
  signal. Their real quota changes still animate. Codex completion is limited to
  explicit rollout events currently observed by the local probe.

## Controls

Settings sidebar icons are unchanged. Notch satellites use a compact three-line
grip and matching sliders. Their separate border-only hover behaviour is retained.

## Updater

The old implementation silently skipped checks because its signing key was a
placeholder, yet the UI could say "Up to date". The replacement starts at "Not
checked yet" and checks only when requested.

Flow: Check GitHub > Download update > SHA-256 verification > Install & restart.
Download progress remains in Settings. Installation uses NSIS passive progress
(`/P /R`), rather than a fabricated in-app installation percentage. On next launch,
the embedded app version must reach the recorded target before success is shown.

The stable release must have exactly one `Matra-Setup.exe`, valid semantic version,
the exact repository/tag asset URL, bounded size and a GitHub SHA-256 digest.
Redirects are restricted to HTTPS GitHub asset hosts. Download content is bounded,
hashed and rechecked immediately before launching. Partial failed downloads are
removed. Only one check/download/install operation can run at a time.

Trust limit: this relies on OS HTTPS trust and GitHub's release metadata. A SHA-256
digest is not publisher signing and does not protect against a compromised release
account or OS trust store. The UI explicitly discloses the unsigned installer.

References: [GitHub asset digests](https://docs.github.com/en/rest/releases/assets),
[Tauri Windows setup](https://v2.tauri.app/distribute/windows-installer/).

## Verification

Final social-link check: 141 native tests passed (5 opt-in tests skipped).
Developer credit links to X; the separate GitHub credit remains clickable.

Pass 1: 140 native tests passed (5 opt-in tests skipped), existing JS tests, UI
parsing and 29 pure motion/update state assertions passed. The separately opted-in
live GitHub check and checksum-verified download passed without executing Setup.

Pass 2: native tests passed again. Isolated rendered Settings/notch fixtures passed
twice and exercise actual button handlers,
progress, errors, explicit install action, confirmation, reading transition,
bounded peek, pointer/carry guards and reduced-motion rendering. Screenshots in
`artifacts/interaction-review/` contain sample data only.

Not yet certified: actual new-version self-update/restart across two released
builds, real-provider timing on every AI, Windows SmartScreen interaction and
physical pointer travel across mixed-DPI monitors. Simulated install confirmation
is not evidence of a completed real self-update.

# Review fixes: preview.12

## Implemented and verified

- **Upgrade handoff:** installer stops only direct version children of the exact
  Programs/MatraNotch root. Verifies new PID/path, binary hash and event-port
  ownership after launch. Real upgrade from running preview.11 to .12 passed;
  independent MATRA-V2 process PID292 survived untouched.
- **Hook isolation:** companion is matra-hook.exe. Ownership parses the command's
  executable, not an arbitrary substring. Only Matra legacy install paths are
  migrated. Nested foreign commands survive install/uninstall. Invalid JSON or
  malformed hooks stop installation; backup failure is fatal; replacement uses
  a sibling pending file. CLI errors now return nonzero. Doctor reports per-event
  owned/foreign command counts. Future installs migrate an already-enabled Matra
  hook to the new executable directory.
- **Machine hook wiring:** all7 events installed,12 foreign commands unchanged.
  Actual companion executable delivered a named test session's running event to
  the app; session_end removed it. Settings backup was retained beside the file.
  Newly installed hooks may need a new Claude Code session to be picked up by the
  host; the running host's hook reload policy was not tested.
- **TLS:** all remote provider adapters now select a shared native-TLS connector
  using OS trust. Certificate/hostname verification remains enabled. Existing
  localhost-only Antigravity self-signed connector is unchanged. Credential-free
  HTTPS probes to ChatGPT and Cursor passed. Installed app then reported fresh
  Codex and Cursor readings, both ok; Codex was not using rollout fallback.
  Historical UnknownIssuer entries remain in the append log; do not mistake a
  whole-file error count for new failures. Issuer logging was NOT added.
- **Hit regions:** hidden satellites no longer submitted. Revealed satellite
  has an explicit small bridge. Removed Rust and JS blanket bounding-box tests.
  Native contract:1 rectangle idle,3 for one revealed handle. Updated Rust test
  proves an unreported wide gap is cold and an explicit bridge is hot.

## Tests

- Rust134 passed,4 opt-in tests skipped in standard run.
- Credential-free live OS-TLS test explicitly run and passed separately.
- HTML script parser passed.
- Ring, theme, independent-handle and hit-rectangle tests passed in installed app.
- Actual Windows cursor test on the current left edge: provider area hidden,
  near border drag only, drag circle retained, return hidden, far border settings
  only, settings circle retained, leaving hides both. No clicks. Original cursor
  restored. Other edges have synthetic contract coverage, not physical coverage.
- Obsolete native-layout-test now delegates current geometry/handle/hit suites;
  physical pointer checks have their own script rather than faked pointer state.

## Still open

Account overflow, uninstall/rollback packaging, keyboard/tray improvements,
real desktop glass, CSP and signed updates. No automatic old-version pruning.
No mixed-DPI/sleep-resume or expired-credential drill. Upgrade failure rollback
is not implemented: installer now reports failure instead of claiming success.
No release-ready verdict, GitHub push or independent re-review is claimed.

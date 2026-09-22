# Mātrā native notch

Adapted from https://github.com/vinzdg/codenotch, Windows subtree at
117a38b8edae2ebd0944bc86b8760c6381685345 (2026-09-22 checkout).
Windows port by Im-Midi and contributors. Original MIT license and notices are
retained in LICENSE. Original notch layout, interaction and provider code retained.

Mātrā customisations: DYDX FX light/dark/system palette, persistent theme command,
separate app identity/configuration/port/startup entry, and branded settings.
Original upstream updater endpoint is disabled for this derivative build.

This Rust/Tauri app replaces the rejected Electron preview as the Windows UI.
It currently uses upstream Rust adapters, not Mātrā's JavaScript adapters. Keeping
one repository does not yet mean the Mac and Windows runtimes share all adapters:
the upstream Mac implementation is Swift. That integration remains explicit work.

## Local Windows build

From the repository root: `npm run native:build`, `npm run native:test`, then
`npm run native:install`. Requires Rust/MSVC and Windows WebView2. The original
`desktop/` Electron experiment is retained but is not the selected Windows UI.

The Start Menu shortcut is Matra Desktop. Launch it again while running to open
Settings, or use the notch settings orb / tray menu. Appearance contains the
DYDX FX System / Light / Dark selector and original folding/edge/size controls.

This is an unsigned local preview. Automatic updates are disabled. Provider
adapters and MIT attribution remain upstream's; no claim of independent authorship.

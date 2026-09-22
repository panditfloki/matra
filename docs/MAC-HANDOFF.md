# Mātrā: same-repository Mac handoff

## Start here

Clone `https://github.com/panditfloki/matra` on the Mac after the approved Windows
candidate is pushed. Check out the exact approved commit or release tag, not an
unrelated upstream CodeNotch release. Read this file, PRODUCT.md, DESIGN.md,
docs/RELEASE-v1.8.3.md and docs/PROVIDER-PORTS.md before editing.

No private Windows folders, credentials, browser profiles or account tokens are
needed. Sign into provider tools independently on the Mac. Apple signing and
notarization credentials must stay in the Mac keychain or protected CI secrets,
never in this repository or a chat handoff.

## Current architecture, not a promise of Mac buildability

- `native/codenotch/` is the active Windows Rust/Tauri app. `ui/` contains the
  current Settings, notch, themes, Logo-18 assets and shared view helpers.
- `native/codenotch/src/extra_providers.rs` contains the new Windows quota readers
  and fixtures. Its JSON parsing and fixed API contracts can be shared; credential
  discovery and CLI executable resolution need platform implementations.
- `native/codenotch/src/usage.rs` defines `UsageSnapshot` / `LimitWindow`.
  Fractions mean USED, reset timestamps are milliseconds, and absent data is not 0%.
- The root Node/extension app, `desktop/` Electron prototype, and `shared/` older
  JavaScript adapters are not the active native Windows product. Do not build the
  prototype and call it Mac parity.
- `upstream-codenotch/` is a local reference, not tracked source. Do not depend on
  it being present after cloning. Upstream reference and pinned commit are in
  docs/PROVIDER-PORTS.md. Keep upstream MIT notices.
- Local `Matra Notch/` and `artifacts/` are ignored. A clone does not contain the
  Windows installer or screenshots from local testing. Published release assets
  are the distribution channel.

## Recommended implementation

Keep one repo with shared Rust quota/parsing modules, shared UI/assets and tests.
Introduce platform modules behind `cfg(target_os)` for:

1. Credential paths and trusted CLI executable discovery (`gh.exe` versus `gh`).
2. Window positioning, pointer hit-testing, tray behavior and native glass effects.
3. Login launch registration (Windows Run key versus the appropriate macOS API).
4. Opening fixed links and folders without Windows `cmd` or Explorer.
5. Updates, installation/relaunch and packaging.

Do not compile Windows-only code unchanged on macOS. Do not simulate Windows
registry behavior on the Mac. A separate Swift shell is an alternative, but shares
assets/contracts rather than the full implementation; it needs a deliberate
architecture decision if actual shared code is the priority.

## Product parity requirements

- Mātrā / DYDXFX branding and supplied logo, four themes, red clickable dydxfx.com.
- Developer: Pandit Floki on https://x.com/panditftw; separate GitHub profile link.
- Side notch centre-up reveal, independent handle hover, reduced-motion support.
- Same quota definitions, status/error semantics and account opt-in behavior.
- Credential reads only after opt-in. Never send one provider's key to another.
- Keep authentication, rate-limits, unavailable data and measured usage distinct.
- Preserve settings across upgrades. Prove login startup and uninstall on the Mac.

## Release boundary

The current updater selects exactly `Matra-Setup.exe` from a stable GitHub release.
It is WINDOWS-ONLY. A Mac updater must select signed Mac assets and implement a Mac
install/relaunch flow. Never download or execute the Windows EXE on macOS.
Keep platform assets in the same release if version parity is intended; publishing
the Mac asset must not replace or delete the Windows installer or checksums.

## Acceptance checks on real hardware

Run shared parser/view tests, then verify signed-in provider readings, notch hit
testing on Retina/mixed-DPI displays, reduced motion, native material effects,
login launch, install/uninstall, and a real old-to-new signed update with restart.
Windows fixture passes do not certify these Mac behaviors.

## Prompt for the Mac agent

> Read docs/MAC-HANDOFF.md and docs/PROVIDER-PORTS.md at the approved Matra commit.
> Implement macOS in this same repository using the current native UI/assets and
> shared provider contracts. First identify Windows-specific modules and propose
> the smallest platform boundary. Preserve Windows behavior and run its shared
> tests. Do not reuse the rejected Electron prototype as the finished product.
> Do not change branding, quota meaning, licenses or release assets silently.

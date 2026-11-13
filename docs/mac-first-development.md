# Mātrā development from the existing Mac product

The product reference is the native Mac app, version 1.9.0/build 1906. Its SwiftUI/AppKit screens, notch and window controllers remain the Mac interface. Windows and Linux use the existing Rust/Tauri desktop target in the same repository, with source-derived Mac geometry and behavior.

The development line is **0.2.0**. Its identities, storage, credentials and explicitly installed hooks are separate from 1.9.0. Development builds do not change production update feeds. The native Swift build is the only Mac packaging entry; the Rust desktop executable refuses to launch on macOS.

## Shared implementation

`native/matra-core` supplies pricing, token aggregation, coverage and Claude quota selection to both the native Swift application and Rust desktop. The C interface and Swift adapter are checked against common vectors and the prior native pricing behavior. Platform credential access, window management and provider transport stay in their native adapters.

`shared/notch-reference/` contains generated Mac geometry. The generator reads the preserved Mac source and rejects unsupported expressions or drift. Windows/X11 carry uses the real body and ring snapshots, perimeter geometry, pointer offset, corner hysteresis, spring integration and measured landing. Release, Escape, capture loss and removed displays have explicit cleanup paths.

## Build and package

Follow [the native build entries](building-matra.md) and [platform dependencies and limitations](development-builds.md). Mac produces native arm64 or x86_64 ZIP/DMG packages. Windows produces an x64 installer; Linux produces x64 Debian and AppImage packages. Package receipts contain identities, versions and SHA-256 values. Hosted development workflows upload artifacts without creating production releases.

## Focused checks

```sh
node scripts/check-mac-notch-reference.mjs --check --self-test --runtime
node native/scripts/test-mac-notch.cjs
node native/scripts/check-ui-scripts.mjs
node native/scripts/test-parity-ui.cjs
node native/scripts/test-carry.cjs
node scripts/build-notch-review.mjs
```

The last command creates an offline component review in `artifacts/mac-first-notch/`. It uses the real Windows renderer and labelled sample data. Its bridge records commands without credentials, provider network access or native window movement.

Browser checks accept existing Puppeteer and Chrome through `MATRA_PUPPETEER_PATH` and `MATRA_CHROME_PATH`. `test-mac-first-browser.cjs` covers 72 geometry combinations, twelve long-list/control cases and four review interactions. `MATRA_BROWSER_WORKSPACE` selects a contained disposable profile/evidence root. Optional `MATRA_BROWSER_SHARDS=4` and `MATRA_BROWSER_SHARD=0..3` split independent combinations across fresh browser processes; run all four to cover the complete matrix. `test-carry-browser.cjs` renders actual carry snapshots and replays Rust spring positions.

Mac tests and explicitly opted-in sample UI use `MATRA_IN_MEMORY_DEFAULTS=1`, which routes app-owned preferences to memory without contacting the system preferences daemon. Environment home redirection alone is insufficient. `native/macos/Scripts/check-defaults-isolation.py` verifies this boundary with daemon access and outside writes denied. Normal app runs retain native preference persistence.

## Acceptance boundaries

Source and browser checks do not establish native WebView2/GTK behavior. Read exact-commit CI package/lifecycle results and review the resulting app on the target device. Physical multi-monitor scaling, native materials, notifications under desktop policy and real-account authentication require their respective environments.

X11 supports the perimeter notch. Wayland restricts global pointer tracking and absolute positioning, so the app opens Settings/Dashboard and explains unavailable controls. Platform-specific credential, terminal and notification limitations are listed in the build guide. Apple hardware-cutout placement remains Mac-specific.

# Mātrā for macOS

Native Swift/AppKit app. Current release: **1.9.0 Beta 6**, build **1906**,
published as **v1.9.0-beta.6**. The release source is under `native/macos`
on [that tag](https://github.com/panditfloki/matra/tree/v1.9.0-beta.6/native/macos).
`main` carries current documentation and the update feed alongside an older
application baseline; use the release tag to reproduce Beta 6.

[Download the Mac beta](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.6).
The DMG contains universal Apple Silicon and Intel code. It uses ad-hoc signing;
Developer ID signing and Apple notarization are pending. Read [INSTALL-MAC.md](INSTALL-MAC.md).

## Build

Requires macOS 15+, Xcode with a macOS 26+ SDK, and XcodeGen. Glass materials
use native APIs on macOS 26 and fall back to solid surfaces on older systems.
The verified toolchain is Xcode 27 beta and XcodeGen 2.44.1. Dependencies are
pinned in Package.resolved; preserve those versions when reproducing the beta.

```sh
git clone --branch v1.9.0-beta.6 --single-branch https://github.com/panditfloki/matra.git matra-beta6
cd matra-beta6/native/macos
make build
make test
make preview
```

Pass `XCODEGEN=/absolute/path/to/xcodegen` when it is not on PATH. Use
`PACKAGE_CACHE=/absolute/path/to/existing/cache` to reuse downloaded Swift packages.
Generated Xcode projects, caches and local build products are ignored by Git.

`make preview` uses sample readings and isolated preferences. It does not start
the live provider readers, connect accounts, start phone linking or enable login
startup. `make run` starts the normal live app and polls only enabled providers.
XCTest hosts exit before live provider discovery. Vendor authorization remains
the user's normal sign-in flow.

To package the tested developer configuration, run:

```sh
Scripts/package-local.sh /absolute/output/directory /absolute/package/cache
```

The release tag's helper produces a local arm64 DMG and checksum. It does not
reproduce the published universal DMG or install/publish the app. Preserve the
pinned dependencies and record the architecture and signing mode of local builds.

## Included in this beta

- Liquid Glass, Dark Glass, Solid Dark, Light and System appearances.
- System clears the app's appearance override and follows macOS immediately.
- Glass ring centres blend into their parent surface; Reduce Transparency and
  older macOS use opaque fallbacks.
- DYDXFX settings, links and branding, plus the Mātrā menu-bar icon.
- Accent controls independent of quota status. Green below Watch, amber at
  Watch, orange at Critical, red from 90%, and neutral unavailable readings.
  Default Watch/Critical thresholds are 50%/70%; both can be adjusted.
- Provider quota cards, supported agent activity, ring placement and visibility.
- Start at login and phone linking remain explicit opt-ins.
- Grok Bot account matching and Cursor protected-credential support. Cursor
  access is requested explicitly through Settings when needed.
- Claude weekly-only readings when the account has no session cap, plus the
  hover-only drag handle and earlier launch/updater corrections.
- The published signed feed offers Beta 6 to builds 1904 and newer through
  **Check for updates**. Older builds require one manual replacement. See
  [installation and recovery](INSTALL-MAC.md) and [feed delivery](../../updates/macos/README.md).

User-owned data uses `Application Support/Matra`, the bundle identifier
`com.dydxfx.matra.mac`, and the app's associated Keychain services. Vendor sessions
remain in their native locations. Credentials and personal preferences are not
part of this source tree or installer.

## Release boundaries

The universal package has passed ARM and Rosetta launch checks on the development
Mac. Rosetta checks do not establish physical Intel Mac acceptance. A wider
hardware matrix, live Cursor access after permission approval, long-duration
stability, phone-client compatibility and the complete installed update/relaunch
remain separate acceptance checks. The Windows app is released separately.
See the [root README](../../README.md) for both channels and the
[maintenance backlog](../../docs/MAINTENANCE-BACKLOG.md) for proposed work.

© 2026 dydxfx · https://dydxfx.com

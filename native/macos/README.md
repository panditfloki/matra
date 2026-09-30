# Mātrā for macOS

Native Swift/AppKit app. Version **1.8.5**, build **185**, published as
**v1.8.5-macos-beta.1** on the repository's `beta` branch.

[Download the Mac beta](https://github.com/panditfloki/matra/releases/tag/v1.8.5-macos-beta.1).
This is the Apple Silicon developer build tested locally. It uses ad-hoc signing;
Developer ID signing and Apple notarization are pending. Read [INSTALL-MAC.md](INSTALL-MAC.md)
and the [release notes](../../docs/RELEASE-v1.8.5-macos-beta.1.md).

## Build

Requires macOS 15+, Xcode with a macOS 26+ SDK, and XcodeGen. Glass materials
use native APIs on macOS 26 and fall back to solid surfaces on older systems.
The verified toolchain is Xcode 27 beta and XcodeGen 2.44.1. Dependencies are
pinned in Package.resolved; preserve those versions when reproducing the beta.

```sh
cd native/macos
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

This produces a local arm64 DMG and checksum. It does not install the app or
publish a release. The signed and notarized public Release configuration has not
been accepted; this beta ships the configuration verified locally.

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
- The automatic-install preference persists and controls automatic acceptance.
  A published signed update feed and the complete downloaded-update transition
  remain pending, so install beta releases manually.

User-owned data uses `Application Support/Matra`, the bundle identifier
`com.dydxfx.matra.mac`, and the app's associated Keychain services. Vendor sessions
remain in their native locations. Credentials and personal preferences are not
part of this source tree or installer.

## Release boundaries

The arm64 beta has been checked on the development Mac. Intel packaging, a wider
hardware matrix, long-duration stability, phone-client compatibility and the
complete automatic-upgrade journey are separate acceptance gates. The Windows
app is developed and released separately. See the root README for both channels.

© 2026 dydxfx · https://dydxfx.com

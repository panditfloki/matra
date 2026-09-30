# Mātrā for macOS 1.8.5 Beta 1

Apple Silicon beta for macOS 15 and later. App version **1.8.5**, build **185**.
Source is on the `beta` branch under `native/macos`. Windows releases remain separate.

## Changes

- Five appearances: Liquid Glass, Dark Glass, Solid Dark, Light and System.
- System follows macOS after switching from an explicit Light appearance.
- Glass icon centres inherit the surrounding material.
- App accent colours personalise settings while quota colours retain their meaning.
- Adjustable Watch/Critical thresholds, with red taking priority from 90% used.
- Updated App & data layout, DYDXFX wording and links, and Mātrā menu-bar artwork.
- Saved automatic-install preference and cancellation behaviour.
- Existing provider readings and supported activity states retained.

## Installation

Download the arm64 DMG and SHA256SUMS.txt from this beta release. Quit Mātrā,
drag Matra.app into Applications, eject the DMG and launch the installed app.
Existing preferences and provider sign-ins are retained. Native glass requires
macOS 26; older supported versions use solid surfaces.

The package uses the locally tested Debug/developer configuration with ad-hoc
signing. It is not Developer ID signed or Apple notarized. An Intel build is not
included. If macOS blocks it, stop and review the warning with protections enabled.

## Verification and remaining work

Publication check on 30 September 2026: **2,046 tests, nine skipped, zero
failures; TEST SUCCEEDED**. An initial snapshot comparison also changed eight
bytes when rendering blue twice, before any accent change. The fixture now
primes the renderer and checks identical inputs as well as blue/red equality
and threshold changes. The final full suite passes with that control.

DMG integrity and deep/strict ad-hoc signature checks pass. The packaged app's
executable matches the working installed app, and an isolated sample-preview
startup remained running during the bounded launch check. No new normal-session
provider or long-duration stability test is claimed by that preview check.

The Mac release tag is excluded from the Windows packaging job so this beta
cannot automatically acquire an unrelated Windows installer.

The signed automatic-update feed and an actual downloaded-upgrade transition
remain pending. Install this beta manually. Wider hardware coverage, phone-client
compatibility and long-duration stability are still acceptance work. The failed
earlier Release-configuration package is not part of this beta.

Windows 1.8.4 remains the downloadable stable version. The next Windows app is
being developed separately; this release contains only the macOS package.

© 2026 dydxfx · https://dydxfx.com

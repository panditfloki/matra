# Install or update Mātrā 1.9.0 Beta 6 for Mac

Build **1906**. The universal DMG contains Apple Silicon and Intel code and
requires macOS 15 or later. Native glass needs macOS 26; earlier systems use
the solid fallback.

## First installation or recovery from a build below 1904

1. Download `Matra-1.9.0-beta.6-macOS-universal.dmg` and `SHA256SUMS.txt` from the
   [Beta 6 release](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.6).
2. Verify the DMG against the checksum, then quit Mātrā from its menu or Settings.
3. Open the DMG and drag Matra.app onto Applications. Choose Replace if an older
   copy exists.
4. Eject the disk image. Launch Mātrā from Applications, not from the disk image.

```sh
shasum -a 256 ~/Downloads/Matra-1.9.0-beta.6-macOS-universal.dmg
```

Existing preferences, usage data and provider sign-ins are retained. Existing
provider permissions may need confirmation after replacing a local build.

**Builds below 1904 need this manual replacement once.** Their installed updater
cannot complete the recovery. The feed gives them an informational update; use
**Settings → View releases on GitHub** or the direct release link above. Repeatedly
trying the old updater does not repair it.

## Updates from build 1904 onward

Use **Check for updates**. The published signed feed offers Beta 6 to eligible
older builds. Downloading a GitHub release and publishing an update feed are
separate steps; both exist for this release. See [Mac update delivery](../../updates/macos/README.md).

Beta 6 retains the earlier launch/updater corrections and Claude weekly-only
readings. It fixes Grok Bot account matching and adds Cursor protected-credential
support. If Cursor needs permission, choose
**Settings → AI sources → Cursor → Allow access…**.

## What has been verified

The final package passed ARM and Rosetta launch checks, and its public download,
checksum, signed feed and archive signatures were checked. A complete installed
update/relaunch and live Cursor access after granting permission remain to be
confirmed. Rosetta testing is not physical Intel Mac acceptance.

The beta is ad-hoc signed and not Apple-notarized. Developer ID signing and
notarization are still pending. If macOS blocks it, review the warning with
protections enabled. A matching checksum or signed feed does not establish
Apple notarization.

© 2026 dydxfx · https://dydxfx.com

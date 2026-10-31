# Mac update delivery

The Mac app's **Check for updates** reads
[`appcast.xml`](https://raw.githubusercontent.com/panditfloki/matra/main/updates/macos/appcast.xml).
Uploading a GitHub release alone does not update this feed. The feed file must be
published on `main`, even when the corresponding application source is on a release branch.

The feed currently offers **1.9.0 Beta 3 (build 1903)**. This explicitly includes the
beta in Mac update delivery; it does not change GitHub's stable/latest release or
the separate Windows updater. The offer is labelled Beta 3 in the app. Existing
Mac installations use the same feed URL and public key, so no bootstrap install
or key rotation is needed. The beta remains ad-hoc signed and unnotarized.

## Every Mac release

1. Build, package and verify the final app, including native launch from the final
   disk image. Increment `CFBundleVersion` for every update, including beta fixes.
2. Publish the immutable archive. Download it anonymously and verify its SHA-256.
3. Sign those exact archive bytes with Sparkle's `sign_update`. Use the existing
   signing key for account `com.dydxfx.matra.mac`; do not generate a replacement.
   `generate_keys --account com.dydxfx.matra.mac -p` prints only its public key.
   It must match the `SUPublicEDKey` in shipped applications.
4. Add the item to `appcast.xml`: numeric build as `sparkle:version`, a clear
   display version, minimum macOS version, public HTTPS archive URL, exact byte
   length and the archive's EdDSA signature. Keep prior compatible items when
   dropping support for older macOS releases.
5. Sign the final XML with `sign_update --account com.dydxfx.matra.mac appcast.xml`.
   Any later edit requires signing the XML again. Verify both the feed and archive.
6. Publish the feed to `main`. Fetch the exact bundled feed URL without credentials;
   require HTTP 200, expected item and signature. Check from an older build and
   confirm the current build reports no newer version.
7. Separately verify download, archive authentication, extraction and the actual
   install/relaunch. Record any untested step instead of treating a feed check as
   proof of a completed installed update.

Never put a private signing key in this repository, build artifacts, logs or shell
arguments. Use the existing local Keychain with Sparkle's signing tools.

See [Sparkle's distribution documentation](https://sparkle-project.org/documentation/).

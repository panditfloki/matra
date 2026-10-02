# Install Mātrā 1.8.5 Beta 2 for Mac

For Apple Silicon Macs running macOS 15 or later. Native glass needs macOS 26;
earlier systems use the solid fallback.

1. Quit Mātrā from its menu or Settings sidebar.
2. Open the macOS arm64 DMG from the v1.8.5-macos-beta.2 release. Build 186.
3. Drag Matra.app onto Applications. Choose Replace if an older copy exists.
4. Eject the disk image. Launch Mātrā from Applications, not from the disk image.

Your preferences, usage data and existing provider sign-ins are not deleted.
The menu bar now uses the approved Mātrā logo. Usage percentages can still replace
that logo when Show limit information in menu bar is enabled.

This packages the tested Debug/developer configuration used on this Mac. It is
ad-hoc signed, not a hardened, Developer ID-signed or Apple-notarized release.
If macOS blocks it, stop and review the warning. Keep macOS protections enabled.
Existing provider permissions may need confirmation after replacing a local build.

Automatic update distribution is a separate release gate. This disk image does
not publish an update feed or enable automatic installations.

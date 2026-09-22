# Mātrā v1.8.3

Release notes and verification record for the Windows build.

## What's new

- Optional GitHub Copilot, OpenCode Go, Command Code and Kimi Code readers, with
  separate connection instructions and opt-in switches. See PROVIDER-PORTS.md for
  credential sources, quota meaning and live-account verification limits.

- Side notch reveals from its centre lip upwards, then opens the lower end over
  500 ms. Rings fade in after the reveal begins. Reduced-motion remains respected.
- Every app startup checks the latest stable GitHub release after a short delay.
  An available update opens Settings with a What's new dialog. No automatic download
  or installation. Manual checking remains available.
- Review release notes, choose Download update, then Install & restart. Update mode
  uses visible Windows Setup progress and preserves existing preferences.
- Startup registration checks the current executable path and Windows disabled
  state. Enabling repairs this app's registration. Duplicate --silent launches no
  longer bring Settings to the foreground.
- Clickable red dydxfx.com in General, with theme-specific contrast. Developer
  credit explicitly says Pandit Floki · X, separately from the GitHub profile link.

## Delivery and verification

An ordinary Git push does not make an update available. The stable v1.8.3
release must contain these notes and the tested Matra-Setup.exe asset plus
SHA256SUMS.txt. The installed v1.8.2 updater then discovers it through
Check for updates.
Automatic startup checking and the notes dialog begin after v1.8.3 is installed.

Tests cover native release selection, bounded notes, startup-check gating, startup
command validation, rendered dialog consent/escaping, links, animation timeline
samples and reduced-motion. Fixtures never execute the installer.

Local checks after provider additions: 150 native tests passed, 5 optional live tests skipped. npm test and
the separate Matra copy check passed. Rendered UI fixtures passed twice; timeline
sampling is deterministic because hidden test windows can suspend presentation.

Not yet verified: a real Windows sign-out/sign-in, actual two-version update/restart,
and physical hover motion in the installed WebView2 window. The installer is still
not publisher-signed. Local tests do not certify these remaining journeys.

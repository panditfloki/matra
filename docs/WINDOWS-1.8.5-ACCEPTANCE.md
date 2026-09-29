# Windows 1.8.5 candidate: ROGPC acceptance

This is the Windows test candidate, not a new stable release. It keeps the
Rust/Tauri + WebView2 implementation and the approved Mātrā visual direction.
The Mac installation is independent and is not modified by this installer.

## Get the exact candidate

Use the successful **Windows Package** run on `codex/windows-185-rc1`.
Download its `Matra-Setup-<commit>` artifact and extract both files together:

- `Matra-Setup.exe`
- `SHA256SUMS.txt`

In that directory, compare `Get-FileHash .\Matra-Setup.exe -Algorithm SHA256`
with the checksum file before opening Setup. Do not use an installer from a
failed run or the public v1.8.4 download when testing this candidate.

The candidate is not publisher-signed. A matching checksum detects a changed
download; it does not establish publisher identity. No antivirus, SmartScreen,
execution policy or other Windows protection needs to be disabled. If Windows
blocks installation, record the warning and stop for review.

## Before installation

1. Keep a copy of the current installer if you want a rollback option.
2. Back up `%APPDATA%\matra-notch` if it exists. Do not delete it.
3. If Claude hooks are enabled, back up `%USERPROFILE%\.claude\settings.json`.
4. Record the current login-start, theme, accent and provider settings.

Run Setup, then launch the installed Mātrā from Start. Confirm that **App & data**
reports **1.8.5**. There should be one active notch, not two installed copies
running together. Setup is an installer, not the app launcher.

## Desktop checks

| Check | Expected result |
| --- | --- |
| Liquid Glass / Dark Glass | Glass icon centres blend with the surface; no solid black disc |
| Solid Dark / Light | Readable controls and provider cards in both appearances |
| Light, then System | Appearance follows Windows, not the previously selected app theme |
| Accent colour | Settings selection and controls change; quota ring colours do not |
| Usage colours | Green below Watch, amber at Watch, orange at Critical, red at 90% |
| Unknown quota | Neutral unavailable state, not a fabricated zero |
| Watch / Critical / ramp | Controls save, reset to 50% / 70% / hard step, survive reopening |
| Hover and movement | Cards open cleanly, notch motion and edge placement remain usable |
| Display scaling | Check ROG display and any external monitor at their normal scaling |
| Login start | Toggle saves; after a Windows sign-out/sign-in it matches the selection |
| Automatic updates | Toggle saves through Settings reopen and a full app restart |
| Real providers | Sign in normally; compare shown usage and reset times with provider sources |

Do not deliberately exhaust a quota to test colours. The regression suite
exercises exact boundary values without consuming account allowance.

## Upgrade and uninstall checks

An existing 1.8.4 install should upgrade without losing preferences, enabled
hooks or login-start choice. Fresh installation must not enable these opt-ins
silently. Uninstall should remove this installation's hooks and login entry,
preserve unrelated hooks/startup entries, and retain saved app data.

The CI lifecycle tests exercise those operations in a disposable Windows
account, including a Unicode installation path. Do not manually recreate its
foreign-hook or malformed-settings fixtures on your personal account.

## What is still a human acceptance gate

CI screenshots use fixture usage data, not signed-in provider accounts. They
cannot prove live account accuracy, physical-desktop animation quality, ROG
mixed-DPI behaviour or Windows login execution. A future published update is
also needed to prove the complete live automatic-download/install journey.

Report a failed check with the app version, Windows version, screenshot and
what you clicked. Redact account identifiers and tokens from diagnostic files.
Do not publish a stable release until the ROGPC checks are accepted.

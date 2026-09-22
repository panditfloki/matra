# Mātrā v1.8.1

AI usage, in proportion.

Download **Matra-Setup.exe** from this release, double-click it, and complete the
Windows installation. No terminal, Node.js, Rust or PowerShell commands needed.

## Included

- Approved DYDXFX Logo-18 in the app, tray, Settings and setup executable.
- Light, Dark, Glass and System appearances; existing theme choice is preserved.
- AI sources, Display and App & data sections with clearer descriptions.
- New mathematical tagline and usage-percentage explanation.
- Upgrade migration for older Mātrā preview processes, shortcuts, enabled startup
  and existing Mātrā Claude hooks. Separate upstream CodeNotch stays untouched.

## Notes

- Windows x64. Installs for the current user without administrator rights.
- The installer is unsigned. Check its SHA-256 against `SHA256SUMS.txt`.
- Glass is translucent; this release does not claim verified Apple-style desktop blur.
- Updates are manual. Download the setup, not the source-code ZIP or `matra.exe`.
- Mātrā uses the MIT-licensed CodeNotch Windows implementation; attribution remains.

## Verification

Native tests: 134 passed, 4 opt-in integration tests skipped. JavaScript tests and
UI parsing passed. A real local upgrade from the running preview verified setup
exit, installed files, the packaged executable bytes, doctor, new-process launch,
hook-port ownership, preserved preferences and unchanged foreign hook commands.
Binary verification accounts for Tauri's documented UNK-to-NSS bundle marker.

## Recovery

Installer migration does not delete old preview folders or reset preferences.
If launch fails, stop the new Mātrā instance and use the previous installer or
retained preview executable. Hook migrations retain a Claude settings backup.

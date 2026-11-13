# Development 0.2.0 builds

The Mac application uses its existing native Swift/AppKit/SwiftUI target. The Rust desktop executable refuses to launch on macOS. Windows and Linux use the Rust/Tauri target in the same repository. No build entry publishes a release or modifies an update feed.

## Entries

Use Node 22.20.0 and Rust 1.97.1. The Windows/Linux packaging CLI is locked to 2.11.4 in `build/development/tools/package-lock.json`.

```sh
npm ci --prefix build/development/tools --ignore-scripts --no-audit --no-fund
node scripts/build-development.mjs --test --platform windows
node scripts/build-development.mjs --bundle --platform windows --tauri-cli build/development/tools/node_modules/@tauri-apps/cli/tauri.js
node scripts/build-development.mjs --bundle --platform linux --tauri-cli build/development/tools/node_modules/@tauri-apps/cli/tauri.js
```

Run the command on its named native OS. Windows and Linux builds target x64. Add `--offline` after dependencies have been provisioned. The dispatcher confines Cargo, temporary files, caches and outputs to the checkout, checks resolved paths, and uses Tauri's local bundler tool directory. `MATRA_BUILD_ROOT` may select another directory inside that checkout. The npm bootstrap should use a contained `npm_config_cache` when run outside CI.

The Mac entry delegates only to `native/macos/Scripts/build-development.py`:

```sh
node scripts/build-development.mjs --test --platform macos --workspace /absolute/contained/work --xcodegen /absolute/xcodegen --packages /absolute/contained/packages --arch arm64
node scripts/build-development.mjs --bundle --platform macos --workspace /absolute/contained/work --xcodegen /absolute/xcodegen --packages /absolute/contained/packages --arch arm64
```

Mac `--check` validates the native entry's command interface only. `--test` runs native tests. `--bundle` requests native tests, Release, ZIP and DMG. The Mac coordinator owns its toolchain and Swift package receipts.

## Isolation and platform behavior

| Area | Development behavior |
| --- | --- |
| Bundle | `com.dydxfx.matra.development020`, version `0.2.0` |
| Settings, cached readings, logs | `matra-development-0.2.0` under the OS config directory; explicit `MATRA_DATA_DIR` must be absolute |
| Event server | Fixed port `48720`; development helper sends an exact channel header |
| Provider keys | Separate hashed slots in Windows Credential Manager or Linux Secret Service; no plaintext fallback |
| Hooks | Explicit opt-in adds only this development helper; production commands are preserved |
| OpenCode | Separate `matra-development020.js`, writing only the development activity namespace |
| Login startup | Explicit opt-in; separate `MatraDevelopment020` Run value or development XDG desktop entry |
| Updates and migration | No production update checks/downloads/install, automatic hook migration or automatic account renewal |
| Linux executable | `matra-development020`, with `matra-development020-hook`; no `/usr/bin/matra` replacement |
| Windows installer | Separate product directory/identity; rejects an existing executable without a development marker; removes only its own opted-in integrations |

Linux X11 supports global pointer sampling, mouse capture, physical desktop coordinates and Escape cancellation. It uses the same Xlib display connection as GTK, so it can replace the application's implicit button grab. Windows capture and capture-loss checks run on the window-owning UI thread. Capture cleanup is queued even after a UI-thread timeout.

Wayland does not offer the global pointer and absolute placement contract required by perimeter carry. Capability flags disable carry and placement controls; native adapter errors do not report a move as successful. Dashboard and Settings remain available. Linux terminal jump-back, foreground acknowledgement, encrypted Cursor editor credentials and the Antigravity ConPTY runner are not supplied by these adapters. Their supported local-tool/cache paths remain separate.

Linux provider keys require libsecret and a desktop Secret Service. Background reads never request unlock, and duplicate slots are rejected. Saving a key is an explicit action; a refused/locked service returns an error. Linux notification submissions require `notify-send` and a notification service. Accepted submission is not proof of visible delivery or permission through Do Not Disturb.

Windows packaging does not fetch a moving WebView2 runtime. A supported installed WebView2 runtime is a host prerequisite and the CI WebView2 test records actual execution. Linux build/runtime prerequisites and the apt snapshot are in the workflow and Debian dependency metadata.

## Validation

`verify-development-inputs.mjs` checks all locked registry dependencies and all packaging CLI platform packages against dated age/integrity receipts. Cargo runs with `--locked`; the workflow pins action commits, Node, Rust and the Linux package snapshot. This supplies repeatable source/tool inputs, not a claim that SDKs, package timestamps or outputs are bit-identical across hosts.

`.github/workflows/development-platforms.yml` runs only the development branch or a manual dispatch. It has read-only repository permission and uploads artifacts. It never attaches releases.

The Windows job tests the installed executable in a disposable runner: Unicode path, explicit hooks/startup, refusal of Windows-disabled startup, synthetic Credential Manager keys, real WebView2 screenshots, saved-settings reinstall, stable-profile/startup sentinels, and owned-integration uninstall. It installs no production baseline.

The Linux job extracts the actual `.deb` inside the workspace, checks packaged executables and linked libraries, starts GTK/WebKit under Xvfb/Openbox with a synthetic fixture profile, captures screenshots, and verifies saved settings after relaunch. This is X11 CI evidence. It is not a physical display, real account, Wayland or user-device acceptance claim. `.deb` and AppImage hashes are written beside the packages.

Local host Rust tests and JavaScript checks do not establish native Windows/Linux acceptance. Read the resulting workflow logs and target-device receipts before calling either platform complete.

API references: [Windows capture thread ownership](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setcapture), [Xlib pointer coordinates and grabs](https://www.x.org/docs/X11/xlib.pdf), [Secret Service search flags](https://gnome.pages.gitlab.gnome.org/libsecret/flags.SearchFlags.html), [Tauri local bundler tools](https://v2.tauri.app/reference/config/).

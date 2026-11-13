# Building Mātrā 0.2.0 development

This repository contains the native Mac app, the Windows/Linux desktop adapter, and a shared Rust business library. Mac continues to use its existing SwiftUI/AppKit screens. It does not run the Tauri interface.

| Directory | Role |
| --- | --- |
| `native/macos` | Existing native Mac interface and OS integration |
| `native/matra` | Windows/Linux interface and OS adapters |
| `native/matra-core` | Actual usage selection, exact model pricing, cost coverage and token weighting used by Swift and Rust |
| `shared/notch-reference` | Geometry and timing generated from the Mac source |
| `scripts/build-development.mjs` | Explicit host-platform build entry |

Development packages are **0.2.0**. They use separate app identities, settings, integration names and local service ports. They do not offer the production update feed or replace an installed 1.9.0 app. A lower development version is not an upgrade for 1.9.0. The root `package.json` version belongs to the separate VS Code extension.

## Windows and Linux

Build on the target OS. The dispatcher refuses a mismatched `--platform` instead of silently making the wrong package. Node 22.20.0, Rust 1.97.1, and Tauri CLI 2.11.4 are the pinned build inputs. Registry dependencies are locked and have a recorded minimum age of fourteen days.

Install the pinned packaging tools into this repository:

```sh
npm ci --prefix build/development/tools --ignore-scripts --no-audit --no-fund
```

Run checks, tests or packages:

```sh
npm run development:check -- --platform windows
npm run development:test -- --platform windows
npm run development:bundle -- --platform windows --tauri-cli build/development/tools/node_modules/@tauri-apps/cli/tauri.js
```

For Linux, replace `windows` with `linux`. Linux builds require GTK3, WebKitGTK4.1, AppIndicator, OpenSSL, Secret Service and X11 development libraries, plus AppImage tooling. The CI workflow installs an explicit Ubuntu snapshot, runs the same build entry, and tests the package under X11. AppImage packaging also needs the normal Linux AppImage build prerequisites.

Packages and SHA256 checksums are written to `artifacts/development/windows-x64` or `artifacts/development/linux-x64`. The Windows output is a separate Development installer. Linux outputs are a Development `.deb` and `.AppImage`.

The workflow **Development Windows and Linux** runs on `dev/mac-first-0.2.0` and supports manual dispatch. It uploads artifacts; it does not publish releases or change update feeds. Its Windows test installs the development package, runs real WebView2 fixture checks, preserves settings across a reinstall and uninstalls it. Linux tests check the extracted package and a native X11 fixture session. A CI result does not establish physical multi-monitor or live-account acceptance.

## macOS

Use Xcode with the SDK needed by the existing native source, existing Rust 1.97.1, and verified XcodeGen 2.46.0. Build output, Swift packages, module caches and hosted test fixtures are placed in an explicit workspace. The Swift package versions are pinned in `native/macos/Package.resolved`. The build script copies a supplied package cache before use, or can use an initially empty `--workspace/packages` directory to resolve the pinned versions.

```sh
npm run development:bundle -- --platform macos \
  --workspace "$PWD/.build/mac-arm64" \
  --xcodegen "$PWD/.build/tools/xcodegen/bin/xcodegen" \
  --packages "$PWD/.build/mac-arm64/packages" \
  --arch arm64
```

The native entry is `native/macos/Scripts/build-development.py`. It creates the Xcode project in `native/macos`, generates development metadata under the workspace, runs the hosted native test suite, builds the unchanged native interface, and packages an ad-hoc signed ZIP/DMG. It does not install the app. Artifacts are under the workspace's `artifacts` directory.

For Intel, use `--arch x86_64` and a Rust 1.97.1 toolchain containing the `x86_64-apple-darwin` standard library. `MATRA_RUSTC` can point at an explicit contained compiler. The core build script never installs or updates toolchains automatically. Native Intel compilation and a Rosetta launch check are different from acceptance on a physical Intel Mac.

Official XcodeGen input: [2.46.0 release](https://github.com/yonaskolb/XcodeGen/releases/tag/2.46.0), archive SHA256 `4d9e34b62172d645eed6457cac13fc222569974098ef4ee9c3368bedf0196806`.

## Development use

Launch the separately named Development app. Sample previews explicitly opt out of account discovery, quota polling, external hooks and login registration. Normal development launches can read enabled local provider accounts; app-owned credentials and settings are separate. Hook installation and startup registration are explicit actions. Background credential renewal and production updater access are disabled for development.

Linux perimeter placement and carry require an X11 session. Wayland does not expose the required global pointer and absolute placement APIs; unsupported controls show the platform reason, and Settings/Dashboard remain usable. Secret storage requires a running, unlocked Secret Service. Missing OS capabilities must return a visible error rather than claim success.

Keep live-account verification separate from fixture tests. Provider authentication, account permissions and vendor endpoint availability can only be confirmed with the accounts used on the target machine.

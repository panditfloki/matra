# Development packaging input audit

Verified on 13 November 2026. This audit covers the selected Windows NSIS and Linux Debian/AppImage build paths. It is a source/input audit, not a claim of bit-for-bit identical hosted VM images or a network-denied packaging run.

| Input | Pin and receipt | Transitive download behavior |
| --- | --- | --- |
| Node/npm | Node 22.20.0, released 24 September 2025, includes npm 10.9.3. Official nodejs.org/dist/index.json and release/v22.20.0 metadata. | setup-node uses that exact Node distribution. No moving Node/npm version is requested. |
| Rust | 1.97.1, minimal profile, distribution manifest dated 16 July 2026; action-age-audit.json. | Explicit toolchain; Cargo.lock fixes 555 registry packages with age receipts and registry checksums. No new crate was added by these packaging fixes. |
| Tauri npm CLI | 2.11.4 and 11 optional native platform packages; tools/package-lock.json SRI plus npm-age-audit.json. | npm ci disables lifecycle scripts, audit and funding. The actual SRI-verified index.js/tauri.js only load the local native binding; missing bindings fail. They do not invoke npm install or fetch another runtime. Node itself is not bundled in Matra. |
| Tauri Linux helpers | Five exact SHA256 inputs in bundler-inputs.json: AppRun, linuxdeploy, GTK/GStreamer scripts and AppImage output plugin. | Tauri's prepare_tools downloads only if these paths are missing; preparer installs fresh verified copies. GTK/GStreamer scripts copy host libraries/resources and call local tools. They contain no network download commands. AppRun/linuxdeploy/plugin embedded tools are covered by their complete asset hashes. |
| AppImage embedded runtime | runtime-x86_64, asset 657374641, updated 19 October 2026, SHA256 617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46. | The output plugin embeds appimagetool. Without an explicit runtime file appimagetool fetches a moving type2-runtime asset. The dispatcher now supplies LDAI_RUNTIME_FILE from the verified target/.tauri/runtime-x86_64. The plugin translates it to --runtime-file. Current moving download URLs fail closed on SHA256 drift. |
| Windows packaging | NSIS 3.11 archive and nsis_tauri_utils 0.5.3 DLL, full SHA256/age receipts in bundler-inputs.json. | Native Tauri NSIS selected downloads are preseeded and verified. webviewInstallMode=skip, createUpdaterArtifacts=false: no WebView2 bootstrapper/offline installer or updater installer download. WebView2 must already be installed; CI uses its pre-existing runtime (131.0.2903.86 in run 43433298721). |
| Linux link/system tools | Ubuntu snapshot 20261029T000000Z. | Every Ubuntu mirror source is disabled before the dated source list is installed. Artifacts include all installed dpkg package versions, exact sources and link-library apt policy. CI 43433298721 confirms GTK 3.24.33-1ubuntu2.2, WebKit2GTK 4.1 2.50.4-0ubuntu0.22.04.1, libsecret 0.20.5-2, X11 2:1.7.5-1ubuntu0.4 and XCB 1.14-3ubuntu3 all match snapshot candidates. |
| GitHub Actions | Full commit SHAs plus age receipts. | Hosted VM/image and action execution runtimes remain externally managed; VM versions appear in CI setup logs. Package source/helper pins are not a claim that the entire hosted machine is immutable. |

The final build must regenerate AppImages after the explicit runtime pin. Earlier AppImages built before this correction are diagnostic artifacts, not delivery candidates.

Primary implementation references:

- Tauri CLI 2.11.4 source commit 8909f221d1515955fc843808032bdc5d62209c96: crates/tauri-bundler/src/bundle/linux/linuxdeploy.rs and windows/nsis/mod.rs.
- AppImage output plugin commit 536b068787179ea901964bd7dabc7bf61e4941c3: src/main.cpp (LDAI_RUNTIME_FILE) and ci/build-bundle.sh (embedded appimagetool).
- https://github.com/AppImage/appimagetool/blob/main/src/appimagetool.c — runtime_file branch versus fetch_runtime.
- https://api.github.com/repos/AppImage/type2-runtime/releases/assets/657374641 — official digest and publication metadata.
- https://nodejs.org/en/blog/release/v22.20.0 and https://nodejs.org/dist/index.json.

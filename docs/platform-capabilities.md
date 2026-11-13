# Mātrā 0.2.0 platform capabilities

Mātrā is developed in one repository. macOS keeps the original SwiftUI/AppKit application. Windows and Linux use the Rust/Tauri desktop target. Both consume the shared pricing, token coverage and quota-selection core. A shared repository does not mean that native window materials or operating-system permissions behave identically.

| Capability | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Interface | Existing native Mac screens and notch | Mac-derived notch geometry, dashboard, settings and source cards | Same desktop interface; perimeter notch requires X11 |
| Carry | Existing native perimeter carry | Native pointer capture and transparent carry overlay | Native X11 capture; unavailable on Wayland |
| Fullscreen folding | Native Mac behavior | Foreground-window monitor coverage | X11 EWMH state; unavailable on Wayland |
| Accounts and quotas | Native provider adapters | Account-scoped provider adapters and credentials | Same adapters where provider tools and OS credential access are supported |
| Costs and activity | Existing native screens; shared calculation core | Dashboard, activity and INR cost views | Same dashboard and calculation core |
| Display choices | Existing native controls | Weekly/daily readings, pace, dashed rings, account labels/order and custom size | Same choices within desktop capabilities |
| Custom endpoints | Existing native endpoint editor | Endpoint mappings, icon presets and bounded PNG uploads | Same endpoint editor |
| Local runtime history | Existing native readers and optional Ollama relay | Count-only LM Studio/oMLX history and opt-in Ollama relay | Same readers and relay; runtime availability depends on the host |
| Notifications | Native Mac channels | System notifications or notch peeks, per-event sounds | Notch peeks on X11; system submission requires a notification service |
| Secret storage | macOS Keychain | Windows Credential Manager | Desktop Secret Service; no plaintext fallback |
| Build output | arm64/x86_64 ZIP and DMG | x64 installer | x64 Debian package and AppImage |

## Platform-specific boundaries

Apple hardware-cutout placement and Liquid Glass remain native Mac features. Windows and Linux preserve the product structure and interactions using their own window systems. Linux Wayland does not grant the global pointer and absolute positioning used by the perimeter notch; the app opens Settings/Dashboard and explains unavailable controls.

Linux adapters do not provide terminal jump-back, foreground acknowledgement, encrypted Cursor editor credential decryption or the Antigravity ConPTY runner. Supported local-tool/cache paths remain available independently. Missing credentials or OS services produce an explicit unavailable/error state.

The Ollama relay observes only clients explicitly pointed to its local address. Direct Ollama traffic is not counted. It forwards conversation data in memory and retains measurements, not prompts or replies. Turning it off stops new requests; a blocked upstream read can remain until its timeout. LM Studio/oMLX history depends on available local logs. Missing history or prices are shown as unknown/partial, not invented zero usage or cost.

Notification submission does not guarantee visible delivery under Focus/Do Not Disturb, muted audio or desktop policy. Live credentials, physical multi-monitor scaling and device-specific desktop behavior require testing on that device.

## Version and update behavior

These packages are a separate **0.2.0 Development** application. They coexist with 1.9.0 and have independent settings and integration names. Check for updates in an existing 1.9.0 app will not install this lower-numbered development channel. Install the development package explicitly.

The package workflow produces artifacts without changing production update feeds. See [build commands](building-matra.md), [native dependencies and lifecycle checks](development-builds.md), and [the Mac-first implementation](mac-first-development.md).

<p align="center">
  <img src="media/brand/matra-logo-18.png" width="76" height="76" alt="Mātrā logo">
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/readme/wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/readme/wordmark-light.svg">
    <img src="docs/readme/wordmark-light.svg" width="480" alt="Mātrā. AI usage, in proportion.">
  </picture>
</p>

<h1 align="center">Your AI limits. Always in view.</h1>

<p align="center">
  See your usage, remaining allowance and reset times at the edge of your screen.<br>
  A quiet meter while you work. The detail when you need it.
</p>

<p align="center">
  <a href="https://github.com/panditfloki/matra/releases/download/v1.9.0-beta.6/Matra-1.9.0-beta.6-macOS-universal.dmg"><img src="docs/readme/download-mac.svg" width="294" height="76" alt="Download Mātrā for Mac, Apple Silicon and Intel, version 1.9.0 Beta 6"></a>
  &nbsp;
  <a href="https://github.com/panditfloki/matra/releases/download/v1.8.6/Matra-Setup.exe"><img src="docs/readme/download-windows.svg" width="294" height="76" alt="Download Mātrā for Windows x64, version 1.8.6 Stable"></a>
</p>

<p align="center">
  <a href="https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.6">Mac release notes</a> ·
  <a href="https://github.com/panditfloki/matra/releases/tag/v1.8.6">Windows release notes</a> ·
  <a href="#install">Installation</a> ·
  <a href="LICENSE">MIT licence</a>
</p>

<p align="center"><sub>Mac: universal Apple Silicon and Intel, macOS 15+, beta, not notarized. Windows: x64, stable, unsigned installer. Mac builds 1904+ can check for updates; older Mac builds and Windows use manual installation.</sub></p>

<p align="center">
  <a href="#how-it-works">How it works</a> ·
  <a href="#your-tools-one-meter">Providers</a> ·
  <a href="#make-it-fit-your-workspace">Appearance</a> ·
  <a href="#choose-your-download">Downloads</a> ·
  <a href="#your-data">Your data</a>
</p>

<p align="center">
  <a href="docs/screenshots/macos-codex-glass.png"><img src="docs/screenshots/macos-codex-glass.png" width="100%" alt="Actual Mātrā Mac preview showing a Codex detail card with allowance, reset credits, token history and a four-provider screen-edge meter"></a>
</p>
<p align="center"><sub>Actual macOS 1.8.5 preview, captured 30 September 2026. Values are historical examples. This capture predates the status-colour update; Windows has its own interface and feature coverage.</sub></p>

## Less checking. More context.

Working across AI tools means keeping track of several allowances and reset windows. **Mātrā** (मात्रा, *measure*) brings supported readings into a compact desktop meter, so you can check them without opening another dashboard.

<table>
  <tr>
    <td width="50%"><strong>Allowance at a glance</strong><br>Rings show usage, with a separate weekly ring where available.</td>
    <td width="50%"><strong>Detail on hover</strong><br>Open a provider card for its limit windows, remaining allowance and reset times.</td>
  </tr>
  <tr>
    <td><strong>Activity beside usage</strong><br>See supported agents working, waiting for you or idle.</td>
    <td><strong>A place on your screen</strong><br>Choose placement, size, visibility and appearance to fit your workspace.</td>
  </tr>
  <tr>
    <td><strong>History when available</strong><br>Codex details include reset credits, token statistics and a 30-day history.</td>
    <td><strong>Honest about missing data</strong><br>Unavailable readings stay unavailable. Cached data is not a promise of live usage.</td>
  </tr>
</table>

## How it works

1. **Install your platform's build.** Windows stable and the Mac beta have separate downloads below.
2. **Choose your sources in Settings.** Reuse supported local sign-ins, or connect the integrations that need their own credentials. Enable the tools you use.
3. **Keep the meter nearby.** Glance at the rings, then hover over a provider for detail. Adjust the layout and appearance to suit your screen.

Mātrā displays what a provider makes available to your account. Fields can differ by plan, tool and signed-in session.

## Your tools, one meter

These integrations are included in the published **Windows 1.8.6** app. The Mac beta has additional source options; a source listed in Settings does not guarantee every metric is available.

| Source | What you can see |
| --- | --- |
| **Claude Code** | Session and weekly limits, reset times and supported activity states |
| **Codex** | Primary/weekly allowances, reset credits and expiry, lifetime/peak tokens, longest chat, streaks and 30-day daily token history |
| **Cursor** | Included usage, API usage and billing-cycle resets |
| **Antigravity** | Reported Gemini, Claude and GPT model-group allowances |
| **Grok** | Weekly allowance from the signed-in CLI session |
| **GLM** | Z.ai Coding Plan utilisation |

**Codex readings cover Codex.** Ordinary ChatGPT browser and app conversations are not included. Quota percentages, token counts and API-price estimates measure different things. An estimated API cost is not your subscription bill.

<details>
<summary><strong>See the Mac source settings</strong></summary>

<p><img src="docs/screenshots/macos-sources-dark.png" width="100%" alt="Mac AI sources settings in dark appearance; the displayed optional connections are disabled or signed out"></p>

Actual macOS 1.8.5 preview from 30 September 2026. This view shows optional source configuration, not connected-account verification. No key or sign-in is supplied with Mātrā.

</details>

## Make it fit your workspace

Choose **Liquid Glass, Dark Glass, Solid Dark, Light or System**. Adjust your meter's placement and appearance, then keep the readings you need in view.

<p align="center">
  <a href="docs/screenshots/macos-display-light.png"><img src="docs/screenshots/macos-display-light.png" width="100%" alt="Actual Mac Display settings in Light appearance, with five themes, reset display choices and weekly ring controls"></a>
</p>
<p align="center"><sub>Mac 1.8.5 preview, captured 30 September 2026. Appearance and controls vary by platform. Native Mac glass requires macOS 26; older supported systems use solid surfaces.</sub></p>

Accent colours personalise the controls. Usage colours communicate how much allowance has been used:

| Default used allowance | Status |
| --- | --- |
| Below 50% | 🟢 Green |
| 50% to below 70% | 🟡 Amber |
| 70% to below 90% | 🟠 Orange |
| 90% and above | 🔴 Red |
| Unavailable | Neutral grey |

Watch and Critical thresholds are adjustable. Red takes priority from 90%. Windows also offers a smooth colour ramp. Notification settings are separate.

## Choose your download

| | macOS | Windows |
| --- | --- | --- |
| **Version** | **1.9.0 Beta 6**, build 1906 | **1.8.6 Stable** |
| **Platform** | Universal: Apple Silicon and Intel · macOS 15+ | Windows x64 |
| **Package** | [Download DMG](https://github.com/panditfloki/matra/releases/download/v1.9.0-beta.6/Matra-1.9.0-beta.6-macOS-universal.dmg) | [Download Setup.exe](https://github.com/panditfloki/matra/releases/download/v1.8.6/Matra-Setup.exe) |
| **Integrity** | [SHA256SUMS.txt](https://github.com/panditfloki/matra/releases/download/v1.9.0-beta.6/SHA256SUMS.txt) | [SHA256SUMS.txt](https://github.com/panditfloki/matra/releases/download/v1.8.6/SHA256SUMS.txt) |
| **Signing** | Ad-hoc signed; not Apple notarized | Not publisher-signed |
| **Updates** | Check for updates from build 1904 onward; one manual replacement for older builds | Run the newer installer |

Download versions checked on **2 November 2026**. Mac and Windows are released separately; Mac screenshots do not establish Windows feature parity. Windows 1.9 remains a [separate preview](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.1); 1.8.6 is the stable Windows download.

### In the current releases

**Mac Beta 6:** corrects Grok Bot account matching and adds support for Cursor's protected credentials. When needed, choose **Settings → AI sources → Cursor → Allow access…** to request access. It retains the Claude weekly-only reading, hover-only drag handle and earlier updater corrections. [Mac release notes](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.6).

**Windows 1.8.6:** the notch keeps its topmost position while preserving focus; settings and drag controls reveal below the capsule; dated Claude readings stay visible when the first refresh is rate-limited. Codex statistics, reset credits and the 30-day graph are retained. [Windows release notes](https://github.com/panditfloki/matra/releases/tag/v1.8.6).

## Install

### Mac

1. Download the **DMG** and its **SHA256SUMS.txt** from the [Mac beta release](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.6).
2. Quit Mātrā, open the DMG and drag **Matra.app** into **Applications**.
3. Eject the disk image and launch Mātrā from Applications.

The universal beta contains Apple Silicon and Intel code and requires macOS 15 or later. It is ad-hoc signed; Developer ID signing and Apple notarization remain pending. If macOS blocks it, stop and review the warning with protections enabled.

Existing preferences and provider sign-ins are retained when replacing the app. **Builds 1904 and newer can use Check for updates** through the published signed feed. **Earlier builds need one manual replacement with this DMG** because their installed updater cannot complete the recovery. Use **Settings → View releases on GitHub** or the download above. See the [Mac installation and update guide](native/macos/INSTALL-MAC.md).

Feed, package and signature checks have passed. A complete installed update/relaunch and live Cursor access after permission approval remain to be confirmed; these checks do not establish Apple notarization or physical Intel Mac acceptance.

<details>
<summary><strong>Check the Mac download</strong></summary>

```sh
shasum -a 256 ~/Downloads/Matra-1.9.0-beta.6-macOS-universal.dmg
```

Compare the result with the checksum supplied in the same release. A matching hash checks file integrity; it does not establish Apple notarization.

</details>

### Windows

1. Download **Matra-Setup.exe** and **SHA256SUMS.txt** from the [Windows stable release](https://github.com/panditfloki/matra/releases/tag/v1.8.6).
2. Run Setup, then open Mātrā from the Start menu.
3. For an existing installation, run the newer installer to upgrade. Preferences and enabled integrations are retained.

The installer is per-user and does not need administrator rights. It is not publisher-signed, so Windows may display a warning. Verify the source and checksum before deciding whether to run it. Updates are manual; an automatic update feed is not configured for this release.

<details>
<summary><strong>Check the Windows download</strong></summary>

```powershell
Get-FileHash -Algorithm SHA256 "$env:USERPROFILE\Downloads\Matra-Setup.exe"
```

Compare the result with the checksum supplied in the same release. A matching hash checks file integrity; it is not a publisher signature.

</details>

## Your data

Mātrā reads supported local sessions, usage records and provider APIs. **No separate Mātrā account is required**, and your usage does not pass through a Mātrā-hosted service.

- Credentials are used with the provider that owns them. Some integrations need an explicit sign-in or API key.
- Readings can be cached, unavailable or limited by an expired session. Freshness varies by source.
- Some provider endpoints are undocumented and may change.
- GitHub is used for release downloads and update-related checks.

The legacy dashboard has separate data paths, including an optional USD/INR rate request. See the [legacy guide](docs/LEGACY-DASHBOARD.md).

## Build and contribute

The desktop apps have separate native implementations. The older Electron experiment in `desktop/` is not the shipping Windows app.

| Platform | Implementation | Start here |
| --- | --- | --- |
| **macOS** | Swift / AppKit | [Mac build notes and pinned dependencies](native/macos/README.md) |
| **Windows** | Rust / Tauri 2 / WebView2 | [Native build notes](native/README.md) · [Windows CI](.github/workflows/windows.yml) |

Use the source tag for the release you want to reproduce: [Mac Beta 6](https://github.com/panditfloki/matra/tree/v1.9.0-beta.6) or [Windows 1.8.6](https://github.com/panditfloki/matra/tree/v1.8.6). The Mac beta source is on its release tag; `main` carries the current documentation and update feed alongside an older application baseline.

<details>
<summary><strong>Build commands</strong></summary>

From a clean checkout of the appropriate release tag, with the documented platform prerequisites installed:

**macOS**

```sh
cd native/macos
make build
make test
```

**Windows**

```powershell
npm run native:test
npm run native:build
npm run native:bundle
```

Keep changes scoped to the relevant platform and preserve dependency pins. A local build is separate from a signed installer or a verified installed update.

</details>

Maintained by [Pandit Floki](https://github.com/panditfloki). The [maintenance backlog](docs/MAINTENANCE-BACKLOG.md) sets out proposed parser regressions, account-isolation tests and maintainer automation, with acceptance criteria and links to existing test evidence. These items are planned work, not completed features.

For bug reports, use [Issues](https://github.com/panditfloki/matra/issues) and include your platform, app version/build and reproduction steps. Remove credentials and personal session content from any logs you share.

**Using the original IDE extension or localhost dashboard?** The existing VS Code-compatible extension and `localhost:4317` dashboard remain in the repository. Their installation, currency settings and architecture are in the [legacy guide](docs/LEGACY-DASHBOARD.md).

<details>
<summary><strong>Screenshot provenance</strong></summary>

The app images on this page are unchanged captures of the macOS 1.8.5 preview from 30 September 2026, previously published with the owner's approval. Their usage numbers are historical examples. The hero's blue arcs predate the status-colour update; current defaults are described above. The source settings image includes disabled or signed-out integrations. These images are not captures of Windows or of every Beta 2 addition.

[Alternate Codex capture](docs/screenshots/macos-codex-glass-alternate.png) · [Brand artwork](media/brand/README.md)

</details>

---

<p align="center"><strong>Mātrā · मात्रा · measure</strong><br>Built by <strong>Pandit Floki</strong> at <strong>DYDXFX</strong>.</p>

<p align="center">
  <a href="https://dydxfx.com">Website</a> ·
  <a href="https://github.com/panditfloki/matra/issues">Report an issue</a> ·
  <a href="https://github.com/panditfloki/matra/releases">All releases</a> ·
  <a href="LICENSE">MIT licence</a>
</p>

<p align="center"><sub>Independent project; not affiliated with the AI providers. Provider names and marks belong to their owners.<br>© 2026 dydxfx · <a href="https://dydxfx.com">dydxfx.com</a></sub></p>

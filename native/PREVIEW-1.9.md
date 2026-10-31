# Mātrā 1.9 · Mac and Windows preview

This preview brings the latest Mac controls and integrations to the native Windows app. Download the two platform builds from the [1.9.0 beta 1 release](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.1). Source is on `preview/matra-1.9.0`; the stable release channel stays separate.

## Test on a ROG PC

1. Quit an older Mātrā instance. Download `Matra-1.9.0-beta.1-Windows-x64-Setup.exe` and compare its SHA-256 with `SHA256SUMS.txt` in the release.
2. Run the installer in your Windows account. It installs the x64 app and preserves existing Mātrā preferences. The installer is not publisher-signed, so Windows may ask you to review the download.
3. Open Mātrā from Start. In **Settings → Display**, turn **Show in system tray** off and on. When hidden, Settings remains reachable through the notch or by reopening Mātrā.
4. Expand the notch. Drag the visible six-dot grip below Settings, without holding a modifier. Test all four screen edges, then your ROG's usual scaling and external display.
5. In **AI sources**, enable the sources you use. New sources start off. Enter optional keys through Settings; they are kept in Windows Credential Manager. Custom endpoints support OpenAI-compatible, Anthropic and Gemini APIs.
6. Open **Dashboard** from the tray or Settings. **Overview** shows current allowances and local usage; choose a source for its details. Open **Project Activity**, or **Settings → Costs**, for account/project filters, calendar navigation and plan settings. Set your INR estimate, plan or custom model prices before relying on cost estimates.
7. **Alerts** starts off. Enable only the events you want. **Send a test** reports whether Windows accepted the notification. Run Mātrā normally from Start: Windows cannot deliver app notifications when it is run as administrator. Do Not Disturb can suppress a banner even after acceptance.

The installer needs Windows 10/11 x64 and WebView2. Windows 11 supplies Mica; other systems use a solid surface. `%APPDATA%\matra-notch` holds the app's preferences. New endpoint credentials do not go into that folder.

## Test on a Mac

Download `Matra-1.9.0-beta.1-macOS-arm64.dmg`. It contains the Apple Silicon app for macOS 15 or later. Quit an earlier copy, then copy the app into Applications. This test build is ad-hoc signed and not Apple-notarized; macOS may require your review in **System Settings → Privacy & Security**.

The Mac app includes the existing dashboard, Project Activity, new provider integrations, the two-way menu bar switch and the always-visible expanded-notch grip. The standalone build uses the installed app identity; the earlier development preview remains a separate identity.

## Controls and integrations

| Area | In this preview |
|---|---|
| Notch | Used/remaining readings, manual/remaining/expiring-quota order, depleted-account handling, optional readings, direct grip drag |
| Windows tray | Show/hide, waiting/busy/completion marks, pulse/wave/off, menu/refresh/activity click action, opt-in usage/reset numeric icons |
| Freshness | Attention and completed-work refresh, with freshness floors and provider backoff |
| Sources | Existing Claude, Codex, Cursor, Grok Build, Antigravity, GLM, Copilot, OpenCode, Command Code and Kimi; optional Grok Bot, Higgsfield, DeepSeek, Ollama, LM Studio, oMLX and llama.cpp |
| Custom endpoints | Protocol selection, model discovery, health, usage adapters/JSON mapping, manual budgets, local discovery and explicit OpenCode provider/model mapping |
| OpenCode | Database-first credential compatibility and an optional metadata-only bridge, installed or removed only through its buttons |
| Activity | Local Claude/Codex per-turn accounting, models, cached tokens, account/project filters, calendar navigation and explicit unknown/partial costs |
| Dashboard | The Mac layout adapted to Windows: Overview, source sidebar, allowance cards, source details, local token/model/project charts, live activity and theme controls |
| Alerts | Opt-in quota thresholds, limits, resets, work-complete and attention notifications, per-provider mute and sound toggle |

## Platform differences and coverage

This is a Windows adaptation with the same main layout and workflows. The Windows system tray uses compact raster readings instead of macOS's wide menu bar text. Window materials, system menus and notification presentation are supplied by each OS. The following data-source and OS-specific differences remain; this is not a claim of pixel-identical controls or complete backend parity.

- **Local runtimes:** Windows shows metrics the endpoint actually reports. It does not include the Mac's LM Studio WebSocket/log history, oMLX log history or Ollama relay activity. oMLX itself runs on a Mac; Windows can monitor that Mac's endpoint. llama.cpp needs `--metrics` for performance readings.
- **DeepSeek:** Windows uses the [documented balance API](https://api-docs.deepseek.com/api/get-user-balance/), not the Mac browser's per-model spend history. Unknown denominators do not become quota rings.
- **Costs:** INR uses a user-editable estimate, not a live FX rate. Missing model prices remain unavailable; partial totals say “At least”. API-equivalent use is not the amount billed. Subscription allocation uses the configured month's price split across this PC's local weighted tokens; it has no other-device or weekly-limit attribution. Weeks start on Monday. These are estimates, not invoices or GST calculations.
- **Accounts:** there is no cross-device credential transport. Sign in to the relevant tools on the ROG PC. Locally discovered tool paths and live-account behavior still need target-device testing.
- **Notifications:** Windows uses its default sound or silence. The Mac's notch notification channel and sound selection are not reproduced. Toast text currently falls back to English. Native CI runs elevated, so its real-process privilege check and disabled-state UI are verified; actual notification delivery needs a normal Windows user session on the target PC. [Microsoft documents the elevation restriction](https://learn.microsoft.com/windows/apps/develop/notifications/app-notifications/app-notifications-quickstart).
- **Ordering:** Windows uses accessible up/down controls for manual account order. Custom icon files can be placed in the app's glyph folder; there is no custom-icon upload picker.

Fixture checks, native Windows CI, installer lifecycle checks and the Mac package checks are distinct from acceptance on the physical ROG PC. Release notes identify the checks that passed for the actual downloadable builds.

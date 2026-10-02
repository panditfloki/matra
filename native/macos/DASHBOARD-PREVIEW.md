# Dashboard and rich menu bar: local test build

This is a local development branch, not a release or a replacement installation.
No GitHub push, tag or release is authorized.

## Three preview modes

| | Sample preview | Local-log preview | Live preview |
|---|---|---|---|
| Launch | `Launch Dashboard Preview.command` or `make preview` | `Launch Dashboard Preview (Local Logs).command` or `make preview-local` | `Launch Dashboard Preview (Live).command` or `make preview-live` |
| Flags | `MATRA_DESIGN_PREVIEW=1` | plus `MATRA_PREVIEW_DATA=local` | plus `MATRA_PREVIEW_DATA=live` |
| Labels | "Sample preview" | "Local-log preview" | "Live preview" |
| Tokens, models, days, Codex projects | Synthetic | **Real** (offline reader) | **Real** (offline reader) |
| Cost | Synthetic, fully priced | **Real offline estimate**, unpriced shown as unavailable | Same as local-log |
| Quotas, resets, reset credits, plan | Synthetic | **Not polled.** Tiles say "Quota · Not polled in preview" | **Real**, polled from your accounts |
| Account discovery and provider polling | Off | Off | **On** |
| Can run beside installed 1.8.5 | Yes | Yes | **No.** It refuses to start while 1.8.5 runs |
| Settings and app storage | Own preview settings | Own preview settings | Own preview settings, own keychain items and support folder |
| Updates, Phone Link, Claude token renewal, login item | Off | Off | Off |

## Live preview: owner procedure

1. Quit the installed Mātrā 1.8.5 (its menu, Quit).
2. From `native/macos`: `make preview-live XCODEGEN=<path to XcodeGen 2.44.1>`.
   Or, after `make preview-build`, double-click
   `Launch Dashboard Preview (Live).command` at the checkout root.
3. Review. The Dashboard and usage panel say "Live preview".
4. Quit the preview with Cmd+Q.
5. Reopen the installed Mātrā 1.8.5 from Applications.

Do not reopen 1.8.5 while the preview runs: both would poll every account.
The preview checks this only when it starts.

What the live preview does and does not touch:

- It runs the normal live app path under the separate identity
  `com.dydxfx.matra.mac.preview`, so its settings are its own. Its own keychain
  items use the `com.dydxfx.matra.mac.preview.*` services and its files go to
  `~/Library/Application Support/Matra Preview`. It never writes the installed
  app's settings, keychain items or `Application Support/Matra` folder.
- It reads the same vendor sign-ins the installed app reads (Claude Code,
  Codex, Antigravity and others). It never renews them: the Claude token
  refresher is off, so if Claude's saved login ages out during a long review,
  run `claude` once in a terminal as usual.
- macOS may ask whether "Matra" may use a keychain item, because this build is
  signed differently from 1.8.5. Click **Allow**, not **Always Allow**:
  "Always Allow" changes that item's access list.
- Providers whose keys or sign-ins 1.8.5 stores itself (MiniMax, Ollama,
  Apify, LM Studio, custom endpoints, DeepSeek and Qianwen web sign-in) start
  unconfigured in the preview. Adding one there stores it only for the preview.
- It starts with fresh preview settings, so What's New and Settings may open
  on first launch, and macOS may ask about notifications for the preview.
- Claude's `/usage` probe runs from `Application Support/Matra Preview/usage-scratch`.
- Updates are never checked or installed; the login-item and auto-update
  switches are disabled; Phone Link never starts.
- The menu-bar item looks like the installed app's when it shows readings;
  the Dashboard, usage panel and window titles carry the "Live preview" label.

### Launch the local-log preview

From `native/macos`:

```sh
make preview-build XCODEGEN=<path to XcodeGen 2.44.1>
make preview-local XCODEGEN=<path to XcodeGen 2.44.1>
```

or, after `make preview-build`, double-click
`Launch Dashboard Preview (Local Logs).command` at the checkout root. That
launcher refuses to start unless the built app's identifier is
`com.dydxfx.matra.mac.preview`, so a build left by `make test` (installed app
identity) is never launched by mistake. The older sample launcher does not
have this check yet (review finding P2-9).

It runs beside the installed 1.8.5 without double polling: it has no quota
reader at all. It only reads local log files, read-only, through the existing
`ccusage --offline` command (no network, nothing installed or fetched):
`ccusage codex daily`, `ccusage codex session` and `ccusage claude daily
--breakdown`, for the last 30 days and the current month. The sample preview
stays available and unchanged.

### What is real in the local-log preview

- Real: tokens per day, per model and per Codex project; the 7-day, 30-day and
  this-month totals; the offline API-price estimate where ccusage has a price.
- Not real or not shown: quota percentages, reset times, reset credits, plan,
  account-wide token history, Gemini and other providers (only Codex and Claude
  have local analytics). "Recent windows" needs a quota reset time, so it says
  it is unavailable here.
- Local logs can include several logins on this Mac. They are not split by account.

### How cost is shown (decided 2026-10-02, D1 = c)

Tokens are always shown. Cost is shown only for priced tokens:

- `~$x`: every token in that figure has an offline price.
- `At least $x`: some tokens have no offline price; `$x` covers only the priced part.
- `Cost unavailable`: none of the tokens have an offline price.
- `Priced, not itemised`: a Codex model with a price. Codex reports cost per
  day, not per model, so a model row cannot have its own cost.
- Recent-window cost is always `At least $x`, because boundary days and today
  are left out.
- "Cost coverage: N% of tokens priced" is weighted by tokens, not days.
- The pricing note names every model with no offline price.
- A combined total says so when one of its parts is an older report whose
  last refresh failed.

Codex rows have no per-model cost, so the app proves which models are priced
from the reports themselves: a day or session costing $0 proves its models
unpriced; a priced row whose other models are all unpriced proves the last one
priced. A model it cannot prove either way counts as unpriced, so the figure
can only err towards "At least". On 2026-10-02 offline ccusage 20.0.19 had no
price for gpt-6-astra, gpt-6-sol, gpt-6.1-sol, claude-opus-5, claude-opus-5-5,
claude-fable-5-1 and claude-sonnet-5-5. A read of the real logs through the new
decoder that day gave:

| Source | Period | Tokens | Shown cost | Tokens priced |
|---|---|---|---|---|
| Codex | 7 days | 530.5M | At least $52.09 (was "~$52.09") | 5.6% |
| Codex | 30 days | 1,496.2M | At least $331.71 | 23.9% |
| Claude | 30 days | 3,741.5M | At least $10.54 | 0.6% |
| Claude | today | 702.8M | Cost unavailable | 0% |

Prices are not fetched and no price table is added; a Mātrā-owned price table
remains a later decision.

## Open safely

After building, double-click `Launch Dashboard Preview.command` at the checkout
root, or run `make preview` here. Both set `MATRA_DESIGN_PREVIEW=1`.
If this preview is already running, use Cmd+1 to reopen its dashboard. Quit the
preview with Cmd+Q before using the launcher again; it is not a second-instance manager.
Do not open the raw build app without that flag for isolated testing: a normal
launch uses real provider readers and the app's existing single-instance policy.

In the sample preview, readings are samples. In the sample and local-log modes, account discovery, provider polling and update
startup do not run. Appearance preferences use the separate existing
`com.dydxfx.matra.mac.design-preview` domain. Your installed app is not replaced.

`make preview-build` uses the distinct local bundle identifier
`com.dydxfx.matra.mac.preview`. The regular build/test targets retain the normal
identifier. Build the preview target after running tests before relaunching the
sample app. This avoids confusing its native app identity with the installation.

## Current rich-panel checkpoint

Left-click the top menu-bar mark labeled **Preview**. It now opens Overview and
provider tabs, with a reading column and an adjacent Token/Cost inspection column.
Cmd+2 is the Open Usage Panel shortcut while the preview is active. Right-click
the top mark for the existing compact utility menu. Cmd+1 opens the larger Dashboard.

The reading column shows quotas, reset times, reset credits when supplied, Today
and 30-day local activity, and recent weekly-window subtotals. The inspection
column offers period selection, Token/Cost charts, selected-day model breakdowns
and Codex project totals. Hover over a provider in Overview to inspect it, then
move into the chart; the last inspected provider remains selected.

The Dashboard uses the same analytics model and components. The popover shares
the existing global theme/accent preferences. Neither surface changes the side
notch's geometry or replaces its compact hover layout. Quota severity colors
remain separate from the app accent and provider chart colors.

Important accounting boundaries:

- The sample preview uses clearly labeled synthetic samples, including its
  Gemini analytics. It never runs the installed offline reader or real polling.
  The local-log preview runs only the offline reader, never quota polling.
- The normal app path can read existing local Codex and Claude logs through the
  already installed `ccusage` command, offline. No package is installed or fetched.
  Reads are shared, triggered by opening/refreshing analytics and later store
  publications, with a 150-second attempt backoff. Manual refresh can retry.
- Local-device tokens/costs are separate from connected-account quotas and Codex
  account-wide token history. Do not add those totals together.
- Costs are USD API-price estimates, not subscription invoices. Unknown pricing
  stays unknown; partial priced totals say "At least", for Codex as well as
  Claude. Codex's daily report has no per-model costs, so Codex model rows say
  "Priced, not itemised" or "Cost unavailable" (see "How cost is shown").
- Weekly-window figures include only recorded, completed days wholly inside the
  inferred reset window. Boundary days and today are excluded. They are lower
  bounds, not exact session/window accounting. Five-hour totals are unavailable.
- Codex project totals cover the loaded 30-day session report. They do not follow
  the selected chart day or period. Attribution uses session metadata cwd, never
  the session's date directory. Claude project attribution is not implemented.
- Live Gemini/Antigravity analytics, INR conversion, event-level window accounting,
  forecasts, Activity, expanded providers and later integrations are still pending.

During earlier same-identity native testing, the existing installed app was found
stopped and was reopened. Its files/settings were not replaced. Installed and sample
preview processes were then verified running separately. Native automation could
list the distinct preview but could not attach to it, so physical top-icon click,
the final preview's visual layout and owner acceptance remain explicit checks.

## What to test

1. Overview shows the sample providers. Open details selects the matching source.
2. The title of a notch hover card opens that provider's dashboard detail. Its
   right-click menu also offers Open details, or Open dashboard outside a card.
3. Dock reopen or Cmd+1 raises one existing dashboard. Cmd+, opens Settings.
4. Switch Liquid Glass, Dark Glass, Solid Dark, Light and System in Settings.
   The dashboard, hover cards and notch follow the same setting immediately.
5. Switch Light to System on a dark Mac. System must clear the previous override.
6. Change the accent. App selections change, quota severity meanings do not.
7. Close Settings while Dashboard remains open, then reverse the order. Closing
   both leaves the sample notch running. Cmd+Q explicitly quits the preview.
8. Resize the dashboard, scroll the sources and details, and reopen after closing.
9. Left-click the top Preview mark. Switch Overview/Codex/Claude, hover a source,
   switch Token/Cost and reporting periods, and select individual chart days.
10. Change the theme while the rich panel is open, then compare Dashboard and
    notch. Open Settings/Dashboard from its footer. Close and reopen the panel.
11. Missing costs must say unavailable/partial, not imply free usage. Recent
    windows must retain their boundary and partial-history explanations.

The initial shell-only checkpoint below is historical. The current slice adds
shared Usage & Spend and Codex project breakdowns as described above. Activity,
new history storage, additional providers, widgets, sync, hooks and plugins remain
on the approved roadmap; they are not counted as finished here.

## Build and validation

### DMC-40 change (honest cost + local-log and live previews): 2026-10-02, NOT BUILT

Not built and not run under XCTest by its author: this floor's sandbox cannot
run `xcodebuild` (package resolution is denied), and that was not worked
around. What was checked:

- `swiftc -typecheck` of `UsageSpendReport.swift` and `UsageSpendReader.swift`
  passed (the two Foundation-only files; the SwiftUI and AppKit files were not
  compiled).
- A throwaway harness compiled those two files and ran 19 decoder checks (the
  new rules plus the existing tests' expectations): all passed. The same
  harness ran the real offline reader on this Mac's logs (figures above).
- `swiftc -typecheck` of `Runtime.swift` and `MatraStorage.swift` passed.
- The new XCTest cases in `Tests/UsageSpendTests.swift` (cost honesty and
  `LocalLogPreviewTests`) have not been run. The live preview path (start
  guard, storage isolation, disabled updater, Phone Link and token renewal)
  has no automated test and has never been launched.

To verify, from `native/macos`:

```sh
make test XCODEGEN=<path to XcodeGen 2.44.1>
make preview-build XCODEGEN=<path to XcodeGen 2.44.1>
make preview-local XCODEGEN=<path to XcodeGen 2.44.1>
# live: quit installed 1.8.5 first
make preview-live XCODEGEN=<path to XcodeGen 2.44.1>
```

Run `make preview-build` after `make test`, since both write the same app path.

### Latest rich-panel evidence: 2026-10-02

- Full regression: 2,072 executed, eight explicit opt-in skips, zero failures.
  Six skips are existing live-provider checks; two are the new offline-reader
  and native status-button checks, which were exercised separately below.
- Affected/native acceptance: 27 tests, zero skips, zero failures. This includes
  actual installed offline Codex/Claude readers, shared theme/model assertions
  and an AppKit status-item popover open/close and size test. A native test is
  not a substitute for the owner's physical status-icon click or visual approval.
- A failed local-reader attempt now backs off for 150 seconds as successful
  attempts do. Manual retry and unrelated providers remain independent.
- Final `preview-build` succeeded with the separate preview identifier.
  Post-build deep/strict signature verification passed, including the existing
  embedded frameworks. This remains a local ad-hoc-signed development build,
  not an installer, notarized package or published release.
- Before the separate-identity rebuild, native Dashboard inspection exposed
  sample Usage & Spend, provider navigation, chart-day selection, model and
  project details. Final separate-identity attachment is blocked in automation;
  no final rich-panel visual acceptance is claimed.

Latest logs: `/private/tmp/matra-rich-menu-retry-regression.log`,
`/private/tmp/matra-rich-menu-retry-acceptance.log`,
`/private/tmp/matra-rich-menu-preview-final.log`.

Final preview executable SHA-256:
`0b1aa0614c90513a2b94b3f0f48c6340967b9fa1d4221d8e81e50c54bd8f62e2`.
Final debug implementation dylib SHA-256:
`e23535de62d42c12ea1f3c56bc37843ab630bf6d95a258df24089a7d262616b1`.

Use the existing XcodeGen-based workflow from `native/macos`. Package pins are
unchanged. XcodeGen 2.44.1 was restored from its official release to a temporary
tools folder, not installed system-wide. The commands used on 2026-10-02 were:

```sh
make test XCODEGEN=/private/tmp/matra-dashboard-tools.ht9gcD/xcodegen/bin/xcodegen
make preview-build XCODEGEN=/private/tmp/matra-dashboard-tools.ht9gcD/xcodegen/bin/xcodegen
codesign --verify --deep --strict --verbose=1 build/Build/Products/Debug/Matra.app
```

After that temporary tool is removed, supply an existing XcodeGen 2.44.1 path
instead. Tests run with the app's test-host isolation, not real provider startup.

### Historical Stage 1 checkpoint: 2026-10-02

- Branch `codex/mac-dashboard-stage1`, base `f8c51e3e9090fe2f6102e26bdfa115a8d8ea2e3a`.
- Baseline: 2,046 executed, nine skipped, zero failures.
- Final: 2,057 executed, six skipped, zero failures, including 11 dashboard tests.
  The six remaining skips require explicit live-provider opt-ins (Amp, Apify,
  Devin Desktop, LM Studio, Ollama and Claude Desktop live-cache rendering).
- Build succeeded. Deep/strict app signature verification passed after the build
  completed, including embedded Sparkle. A fresh launch of that final app succeeded.
  This is a local development build, not a notarized distribution artifact.
- Native preview: Overview and Codex detail open; reset credits, token history
  and full-width quota bars render. Unknown Gemini usage remains unknown.
- Native preview: Liquid Glass, Dark Glass, Solid Dark, Light and System were
  exercised. Light -> System on this dark Mac changed Settings and Dashboard
  back to dark immediately. The OS appearance preference itself was not changed.
- Cmd+1 and Cmd+, reuse the dashboard and settings. Closing either leaves the
  other usable. Closing both leaves the notch; reopening retains Codex selection.
  The notch context menu's Open Dashboard action reopened the dashboard.
- Native inspection caught narrow quota tracks in the larger window. The fix is
  dashboard-only opt-in layout; the default compact hover layout is unchanged.
- Automated tests cover both simulated OS appearance directions, shared-store
  updates and coalesced refresh, account removal, retained selection, Reduce
  Transparency, multi-window activation policy and expanded-track rendering.

Logs: `/private/tmp/matra-dashboard-baseline.log`,
`/private/tmp/matra-dashboard-layout-tests.log`,
`/private/tmp/matra-dashboard-final-build.log` and
`/private/tmp/matra-dashboard-final-preview.log`.

Initial Stage 1 executable SHA-256 (superseded by the follow-up below):
`2f6379bbd2f39dd6eaa2ba381d844e91212441b666c0cb5a5b5ac929f117e0fc`.
Debug implementation dylib SHA-256:
`75c00a13fb1f5ac6d2ddda655948056173d09a3378fd73d5d8569369eb365cd0`.

### Still needs acceptance

- Physical Dock click and hover-card title click. Dock automation timed out;
  shortcut and notch context-menu success are not substitutes for those checks.
- Live-account comparisons, physical monitor removal/mixed displays, long-session
  resource use and comprehensive keyboard/accessibility testing.
- Independent code review and the owner's visual acceptance.
- Later roadmap stages and Windows parity. No Windows source was changed here.

The preview is left open in Liquid Glass with Show on hover restored. Preview
preferences are separate from the installed app. No app replacement, commit,
push, release, updater-feed change or signing-account change was performed.

## Top menu-bar entry

The macOS menu bar is the strip at the top of the screen, not the Dock.
In the sample preview, open Settings -> Display -> App preferences -> App icon
and select Menu bar. Left-click the top Preview mark for the rich usage panel;
right-click for the menu labeled "Mātrā Preview · Sample data only". Neither can
refresh real providers or start Phone Link. Dock/Menu bar/Neither retain the existing app
choices; opening a foreground window can temporarily show a Dock tile too.

The initial Stage 1 sample harness omitted this status-item wiring even though
the normal app path already had it. The 2026-10-02 follow-up corrects that omission
and adds a regression test for appearance/disappearance and Dashboard navigation.

Historical menu-entry follow-up: 2,058 tests executed, six opt-in skips, zero failures,
including all 12 dashboard tests and the existing Settings appearance test.
Build and post-build deep/strict signature verification passed. The rebuilt
sample preview freshly launched with Menu bar selected; Cmd+1 opens Dashboard.
Actual top-status-icon click is still an owner acceptance check because macOS
menu-bar automation timed out. A selected preference alone is not visual proof.

Test isolation note: early full runs failed two assertions in the existing
System-theme test after the new test created real menu-bar UI. The combined
dashboard/theme subset passed, and a full diagnostic run omitting the new test
passed. Injecting native presentation for the test, without removing the new
test or weakening the theme assertions, restored a passing full suite. Native
menu creation/click acceptance is intentionally separate from that unit test.

Historical menu-entry logs: `/private/tmp/matra-dashboard-menubar-final-tests.log`,
`/private/tmp/matra-dashboard-menubar-build.log`,
`/private/tmp/matra-dashboard-menubar-native.log`.
Historical menu-entry executable SHA-256:
`80003eff1030c3436cf771b5cc0daf14064eb07fbc2785925ec8ead0967f9a22`.
Historical menu-entry debug implementation dylib SHA-256:
`4de6674e103fbac967f78bc8de68e49e6d3ba6d72d2f7f057326e68a228fd499`.

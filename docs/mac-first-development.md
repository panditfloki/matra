# Mātrā: development from the existing Mac product

The Mac app is the product reference: version 1.9.0, build 1906, source release
`121746dcf4a0e619fa05ddd9b81f9b2e64f00da1`. Its SwiftUI/AppKit interface stays in
place. This branch develops the Windows equivalent against that reference.
The earlier replacement Mac preview is not part of this implementation.

The development line is **0.2.0**. This first milestone is source and a browser
component proof, not a new native installer or an update for installed 1.9.0.
Native package versions and update-channel migration belong to the later,
separately validated packaging step; no existing feed is changed here.

## First component

The Windows notch uses the original Mac measurements for its silhouette, rings,
provider spacing, cards and settings/move controls. A generated reference in
`shared/notch-reference/` gives adapters stable names for those measurements and
timings. The generator reads the existing Mac source and fails on unsupported
expressions or drift. The browser module embeds the same generated data.

This is shared reference data. It is not yet a shared provider/cost runtime.
Mac still executes its current Swift source. A later core extraction must keep
its current interface and behavior while introducing small, testable boundaries.

## Local checks

From the repository root:

```sh
node scripts/check-mac-notch-reference.mjs --check --self-test --runtime
node native/scripts/test-mac-notch.cjs
node native/scripts/check-ui-scripts.mjs
node native/scripts/test-parity-ui.cjs
node scripts/build-notch-review.mjs
```

The last command writes `artifacts/mac-first-notch/index.html`. Open it in a
browser to interact with the actual Windows renderer using labelled sample data.
The file is self-contained. The sample bridge records commands but does not
connect to providers, move native windows, change settings or run updates.

For real browser geometry checks, reuse an installed Puppeteer and Chrome:

```sh
MATRA_PUPPETEER_PATH=/absolute/path/to/puppeteer \
MATRA_CHROME_PATH=/absolute/path/to/chrome \
node native/scripts/test-mac-first-browser.cjs
```

All profiles, output and temporary browser data stay in the enclosing task
directory. No dependencies are installed by these commands. The browser check
uses the real HTML and scripts, checks four edges, multiple provider counts,
three notch sizes, two pixel ratios, hover content and command boundaries.
Additional ten-provider cases cover normal mouse-wheel scrolling, taskbar
insets, hidden readings and last-provider hover/refresh. It does not replace
native Windows/WebView2 or physical pointer testing.

The Windows CI job also checks reference drift and adapter geometry. Its compiler
is explicitly pinned to Rust 1.99.1; it does not follow moving stable. A configured
workflow is not evidence that Windows CI has run for this branch.

## Remaining platform work

- Compare the running Mac and Windows component with the owner. Fonts, measured
  line heights, glass materials, DPI and animation are platform dependent.
- Finish the resting settings arc's native flare-following geometry and exact
  spring/shape transitions. The current ordinary silhouette port and CSS timing
  preserve reference dimensions but do not establish identical motion.
- Port native carry behavior: Mac follows the pointer around the display with
  corner hysteresis and springs; the current Windows native command still uses
  an edge chooser. Matching handle geometry does not complete this interaction.
- Verify physical hover/hit regions, screen changes and taskbar insets in Windows.
  The fixed Windows overlay also compresses/scrolls long provider lists earlier
  than the Mac's screen-sized layout; ordinary mouse-wheel access is checked.
- Extend the established pattern to dashboard/settings/provider behavior and
  extract reusable business logic incrementally. Preserve the existing Mac UI.
- Add Linux adapters and native build/test evidence. This component alone does
  not establish Linux readiness or complete Mac feature parity.

The ordinary display-edge notch is the scope of this slice. Mac hardware-cutout
geometry and native window behavior remain separate, explicit platform concerns.

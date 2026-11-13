# Mac notch reference data

`mac-notch-reference.json` is generated from the existing native Mac notch. It
gives Windows and future adapters one named reference for dimensions, typography
inputs, motion parameters, hover grace, sizes and edge anchors. Mac continues to
use its existing Swift presentation and code. This is reference data, not a
shared runtime or a completed platform migration.

From the repository root, using Node and no dependencies:

```sh
node scripts/check-mac-notch-reference.mjs --check --self-test
node scripts/check-mac-notch-reference.mjs --check --runtime
```

After reviewing an intentional Mac change, regenerate the JSON:

```sh
node scripts/check-mac-notch-reference.mjs --write
```

The browser adapter embeds this same JSON between
`/* BEGIN GENERATED MAC REFERENCE */` and `/* END GENERATED MAC REFERENCE */`.
Its generated literal must be refreshed when the contract changes. `--runtime`
checks that literal against the current source-derived JSON. The generator never
writes the adapter or native Mac files.

## Contract

- `design` records points per design-frame pixel and SF Pro's cap-to-em ratio.
- `designPixels` preserves the original `Design.px(...)` inputs.
- `layout` uses Mac logical points and the original `NotchLayout` field names.
  `localArcMinimumSweep` and `orbMergeScale` are dimensionless. `maxWindowCount`,
  `sessionCeiling` and `defaultSessionCap` are counts.
- `typography` gives cap measurements and computed font sizes, not font metrics.
- `motion` preserves Swift spring response/damping and ease durations in seconds.
  A CSS easing curve is an adapter choice, not proof of an identical spring.
- `shape` gives the ordinary silhouette's curve reach, bend ramp and integration
  step count. Its algorithm remains in the native shape and platform adapter;
  the full native shape source is fingerprinted to detect algorithm changes.
- `rendering` gives bezel bleed, content-retreat distance, ring dash lengths in logical
  points, activity-arc fraction, and ring/glyph opacities in normal and reduced
  transparency modes. Visibility and data-state logic remain adapter behavior.
- `interaction` gives hover-out, fold-out and settings-return durations in seconds;
  `gripGrowth` is the dimensionless additional hover growth.
- `sizes` contains the native small/medium/large notch scales.
- `anchors` describes top-left panel coordinates, with y growing down. `along`
  runs down side edges and right across horizontal edges. `outward` points toward
  the bezel; tooltip direction points inward. `bezelAnchor` is the native scale
  anchor. These are logical directions, independent of host window APIs.
- `provenance` fingerprints every read source and the existing Mac expectation
  tests. It intentionally has no timestamp or current branch commit, so output
  is deterministic for identical inputs.

The small parser accepts finite decimal arithmetic, known layout constants and
`Design.px(...)` only. Source syntax outside its supported subset, missing or
duplicate fields, unsupported edge mappings and unknown names fail. It does not
evaluate Swift or silently retain old values. Changed source fingerprints fail
the drift check even if the change only affects a comment or an unextracted part
of a file. Review that change before regenerating.

Independent checks preserve existing native-test constraints: the 44-point ring,
body/card ratios, activity and weekly-ring clearances, minimum visible arc,
stagger cap, reduced motion, and inward tooltip directions. The source tests are
fingerprinted and their named cases are required. These Node checks do not claim
that the native XCTest suite has run.

## Deliberate boundary

AppKit measures percent/title/body line heights and text wrapping at runtime.
Screen/cutout geometry and the live model determine fitted spacing, card height,
hardware-notch joins and drag behavior. Those values are listed under
`nativeMeasured`; they are not guessed as portable constants. An adapter must
measure its own text and screen and test its placement. Fonts, physical DPI,
SwiftUI springs, glass rendering, native input and actual device appearance still
need platform acceptance. The card is not scaled with the notch in the current
native implementation; use its logical dimensions independently of `sizes`.

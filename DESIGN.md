# Mātrā visual reference

Display name: `dy/dx.f(Mātrā)`, confirmed by user. Product files on Windows use
`dy-dx.f(Mātrā)` because slash is not permitted in filenames. Developer credit:
Pandit Floki, https://github.com/panditfloki; original MIT acknowledgments retained.

Appearance modes: Light and Dark are solid; System follows Windows with solid
surfaces; Glass uses a smoked warm-grey tint, independent of the OS theme. It
remains translucent, not native desktop blur. Idle black
brackets and opaque inverse corner fillets are removed. Settings and move controls
use approved option D: separate 32px circular satellites, centered above/below
the capsule with a 2px gap (left/right on horizontal edges). They appear only
on hover, with a 300ms exit grace period, and disappear when the notch folds.

Preview.11 refines reveal: hovering provider content reveals neither control.
The near10px end border reveals drag only; the far10px end reveals settings only.
Each bridge/circle maintains that control's reveal. Fade160ms/scale180ms, respecting
reduced motion. Existing settings gear SVG unchanged.

Preview.9 compact geometry: resting lip 5px deep (previously 10px), 79px long.
Expanded vertical body 62px wide, ring gap 10px, end padding 14px.
Horizontal body uses 8px cross-axis padding. Ring and text sizes unchanged.
All dimensions follow existing Small .8 / Medium 1 / Large 1.25 scaling.

Preview.10 quota rings: independent current and weekly colours. Used quota below
50% emerald, 50–74% amber, 75–89% orange, 90%+ red. Unknown/non-metered values
never get a quota-warning arc. Opaque ring backing protects contrast on Glass;
deep shades on Light/Glass, bright shades on Dark. Main stroke 5.1 SVG units
(4px at the 44px ring size), weekly 3.2 units (2.5px). Activity stays neutral.

Source inspected 2026-09-22: https://dydxfx.com/styles.css?v=67 and home page.

Dark: canvas #0a0908, elevated #131110, card #1a1714, text #f8f4ed,
secondary #b6ab9b, accent #cc785c, on-accent #1c0f09.
Light: canvas #faf8f6, elevated #ffffff, card #f4f1ed, text #14110f,
secondary #57504a, accent #d93f2a, on-accent #ffffff.

Website type: Inter for body, JetBrains Mono for technical labels and logotype.
Use local fallbacks when unavailable; do not claim bundled fonts that are absent.
Logo variables are italic, operators upright and accent-coloured. Not Georgia.

Retain native CodeNotch geometry. Apply the chosen theme to folded and expanded
notch, SVG ring backgrounds/tracks, handles, hover details and settings together.
Requested glass treatment uses translucent tinted surfaces, subtle rim highlights
and existing native settings Mica. CSS backdrop filtering does not guarantee
blur of other desktop windows through WebView2. No macOS Liquid Glass parity claim.

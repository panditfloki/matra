import AppKit
import SwiftUI

/// DYDXFX warm neutrals. Dark materials use ivory/sand ink; legacy Light and
/// System retain their light palette. Quota bands live separately in MatraBrand.
/// Glass backings are explicit warm-charcoal washes, not wallpaper-blue fills.
enum Palette {
    static let notch         = Color(dark: NSColor(hex: 0x0a0908), light: NSColor(hex: 0xfaf8f6))
    static let card          = Color(dark: NSColor(hex: 0x131110), light: NSColor(hex: 0xffffff))
    static let ringTrack     = Color(dark: NSColor(hex: 0x49413a), light: NSColor(hex: 0xd4cbc2))
    static let barTrack      = Color(dark: .white.withAlphaComponent(0.176),
                                     light: .black.withAlphaComponent(0.15))

    /// Named so `UsageBand.rampColor` can interpolate between them per-appearance rather
    /// than blending two already-resolved `Color`s (which would mix in whichever appearance
    /// happened to be current when the `Color` was built, not the one it draws in).
    static let amplePair: (dark: UInt32, light: UInt32) = (0x00FF88, 0x00A356)
    static let watchPair: (dark: UInt32, light: UInt32) = (0xF2FF00, 0xB08800)
    /// Already 3.5:1 on white, so the warning colour is the same in both.
    static let criticalPair: (dark: UInt32, light: UInt32) = (0xFF3F00, 0xFF3F00)

    static let ample         = Color(dark: NSColor(hex: amplePair.dark), light: NSColor(hex: amplePair.light))
    static let watch         = Color(dark: NSColor(hex: watchPair.dark), light: NSColor(hex: watchPair.light))
    static let critical      = Color(dark: NSColor(hex: criticalPair.dark), light: NSColor(hex: criticalPair.light))

    /// A continuous point between two palette anchors, each resolved for the current
    /// appearance first and interpolated in sRGB channels second — resolving after
    /// interpolating would blend whichever appearance was current when the ramp was
    /// evaluated into every later draw, not the appearance it is actually drawn in.
    static func ramp(from: (dark: UInt32, light: UInt32), to: (dark: UInt32, light: UInt32), fraction: Double) -> Color {
        let t = min(max(fraction, 0), 1)
        return Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return NSColor(hex: isDark ? from.dark : from.light)
                .blendedByChannel(with: NSColor(hex: isDark ? to.dark : to.light), fraction: t)
        })
    }

    // Generation-speed bands are independent of cloud quota usage.
    static let generationFast = Color(hex: 0x0A84FF)          // blue
    static let generationSlow = Color(hex: 0xFF453A)          // red

    /// The one wash of our own laid *under* the system's glass, and only for
    /// `NotchSurfaceStyle.darkGlass`: that style exists because a black notch
    /// was asked for regardless of the Mac's Appearance, which the untinted
    /// `glass` cannot promise. It is a background beneath `Glass.clear`, not a
    /// `Glass.tint` — tinting only colourises an adaptive material and made the
    /// surface lighter, so the darkening has to happen behind the glass. 0.45
    /// was the first value tried and read too light through `Glass.clear` on a
    /// real Mac (macOS 27) against a light desktop; 0.60 is what the user
    /// chose by eye instead — opaque enough to be the bezel, thin enough not
    /// to be `solid`.
    static let glassCharcoal = Color(hex: 0x0a0908)
    static let darkGlassDim = glassCharcoal.opacity(0.60)

    /// Tooltip copy retains the frame's #808080 secondary ink in Dark glass.
    /// It needs a deeper local backing than the notch itself when a light
    /// desktop is visible through `Glass.clear`, otherwise the two greys merge.
    static let darkGlassTooltipDim = glassCharcoal.opacity(0.80)

    /// A tooltip-only wash for standard Liquid Glass in dark appearance. It is
    /// intentionally weaker than `darkGlassDim`: regular glass stays visibly
    /// distinct from the user-selected always-dark surface.
    static let liquidGlassTooltipDim = glassCharcoal.opacity(0.35)

    static let textPrimary   = Color(dark: NSColor(hex: 0xf8f4ed), light: NSColor(hex: 0x14110f))
    static let textSecondary = Color(dark: NSColor(hex: 0xb6ab9b), light: NSColor(hex: 0x57504a))
    /// Used only by dark, standard Liquid Glass tooltips. Other surfaces keep
    /// `textSecondary`, including their frame-accurate #808080 dark ink.
    static let readableTooltipTextSecondary = Color(dark: NSColor(hex: 0xd6cdc0), light: NSColor(hex: 0x57504a))
}

extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red:   Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue:  Double(hex & 0xFF) / 255,
            opacity: 1
        )
    }

    /// Resolved against whatever appearance is current when the colour is
    /// drawn, which for the notch is the panel's: `nil` for glass (so the Mac
    /// decides) and `darkAqua` for solid.
    init(dark: NSColor, light: NSColor) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
        })
    }
}

extension NSColor {
    convenience init(hex: UInt32) {
        self.init(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green:   CGFloat((hex >> 8) & 0xFF) / 255,
            blue:    CGFloat(hex & 0xFF) / 255,
            alpha:   1
        )
    }

    /// A linear per-channel sRGB lerp — the same arithmetic the Windows ramp does in JS, kept
    /// deliberately simple rather than going through `blended(withFraction:of:)`, whose
    /// blending colour space is not something either side of a Mac/Windows parity claim
    /// should depend on.
    func blendedByChannel(with other: NSColor, fraction: CGFloat) -> NSColor {
        guard let a = usingColorSpace(.sRGB), let b = other.usingColorSpace(.sRGB) else { return self }
        func lerp(_ x: CGFloat, _ y: CGFloat) -> CGFloat { x + (y - x) * fraction }
        return NSColor(
            srgbRed: lerp(a.redComponent, b.redComponent),
            green:   lerp(a.greenComponent, b.greenComponent),
            blue:    lerp(a.blueComponent, b.blueComponent),
            alpha:   1
        )
    }
}

private struct MatraReduceTransparencyKey: EnvironmentKey {
    static let defaultValue: Bool = false
}

extension EnvironmentValues {
    /// True when macOS Accessibility "Reduce Transparency" is enabled in system settings,
    /// or explicitly overridden via `.environment(\.matraReduceTransparency, ...)`.
    var matraReduceTransparency: Bool {
        get { self[MatraReduceTransparencyKey.self] || self.accessibilityReduceTransparency }
        set { self[MatraReduceTransparencyKey.self] = newValue }
    }
}

private struct MatraHeadlessGlassKey: EnvironmentKey {
    static let defaultValue: Bool = false
}

private struct TooltipSecondaryInkKey: EnvironmentKey {
    static let defaultValue = Palette.textSecondary
}

extension EnvironmentValues {
    /// Secondary ink resolved for the current tooltip surface. This stays
    /// frame-accurate unless ordinary dark Liquid Glass needs extra contrast.
    var tooltipSecondaryInk: Color {
        get { self[TooltipSecondaryInkKey.self] }
        set { self[TooltipSecondaryInkKey.self] = newValue }
    }
}

extension EnvironmentValues {
    /// Draw the glass path with the system material left out. Tests only; the
    /// app never sets it.
    ///
    /// `ImageRenderer` cannot draw the material faithfully: in a cold process
    /// it paints `glassEffect` as nothing at all, and once any test has shown
    /// a live `NotchPanel` it paints it as an opaque flat grey over its ZStack
    /// siblings for the rest of the process. Either way the pixels say nothing
    /// about the product. So the glass pixel tests render everything *around*
    /// the material — the transparent body fill, the `darkGlass` dim, the
    /// opaque hardware band — which is the part that is ours to get wrong.
    /// See TASKS.md, "The hardware's band stays black".
    var matraHeadlessGlass: Bool {
        get { self[MatraHeadlessGlassKey.self] }
        set { self[MatraHeadlessGlassKey.self] = newValue }
    }
}

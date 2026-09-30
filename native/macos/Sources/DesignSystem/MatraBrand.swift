import AppKit
import SwiftUI

/// Presentation identity only. Provider IDs and data contracts stay native.
enum MatraBrand {
    static let name = "Mātrā"
    static let displayName = "dy/dx.f(Mātrā)"
    static let website = URL(string: "https://dydxfx.com")!
    static let github = URL(string: "https://github.com/panditfloki")!
    static let repository = URL(string: "https://github.com/panditfloki/matra")!
    static let author = URL(string: "https://x.com/panditftw")!

    /// Brand presentation copy without rewriting dynamic URLs or file paths.
    static func copy(_ text: String) -> String {
        let protected = try! NSRegularExpression(pattern: #"https?://\S+|(?:[A-Za-z]:)?[/~]\S+|(?:[\w.-]+/)+[\w./~-]+|[\w.-]+\.\w+"#)
        let brands = try! NSRegularExpression(pattern: #"\bmatra\b"#, options: .caseInsensitive)
        let range = NSRange(text.startIndex..., in: text)
        let protectedRanges = protected.matches(in: text, range: range).map(\.range)
        var result = text
        for match in brands.matches(in: text, range: range).reversed() {
            guard !protectedRanges.contains(where: { NSIntersectionRange($0, match.range).length > 0 }),
                  let swiftRange = Range(match.range, in: result) else { continue }
            result.replaceSubrange(swiftRange, with: name)
        }
        return result.replacingOccurrences(of: " \u{2014} ", with: ", ")
            .replacingOccurrences(of: "\u{2014}", with: "-")
    }

    static let accent = Color(dark: NSColor(hex: 0xcc785c), light: NSColor(hex: 0xd93f2a))
    static let onAccent = Color(dark: NSColor(hex: 0x1c0f09), light: NSColor(hex: 0xffffff))
    static let elevated = Color(dark: NSColor(hex: 0x131110), light: NSColor(hex: 0xffffff))
    static let card = Color(dark: NSColor(hex: 0x1a1714), light: NSColor(hex: 0xf4f1ed))
    static let border = Palette.textPrimary.opacity(0.12)
    static let smoke = Color(hex: 0xb7ada3)

    /// Keep button text readable even when the user chooses a very light accent.
    static func readableInk(on color: Color) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            var ink = NSColor(hex: 0xfaf8f6)
            appearance.performAsCurrentDrawingAppearance {
                guard let rgb = NSColor(color).usingColorSpace(.sRGB) else { return }
                func linear(_ channel: CGFloat) -> CGFloat {
                    channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
                }
                let luminance = 0.2126 * linear(rgb.redComponent)
                    + 0.7152 * linear(rgb.greenComponent) + 0.0722 * linear(rgb.blueComponent)
                ink = NSColor(hex: luminance > 0.179 ? 0x14110f : 0xfaf8f6)
            }
            return ink
        })
    }
}

/// Semantic usage colours never inherit the app's decorative accent.
/// Watch and Critical follow the user's limits; 90% is always near exhaustion.
enum MatraQuotaBand: String {
    case unknown, emerald, amber, orange, red

    static func band(for fraction: Double?, watchLimit: Double = 0.50,
                     criticalLimit: Double = 0.70) -> Self {
        guard let fraction, fraction.isFinite else { return .unknown }
        if fraction >= 0.90 { return .red }
        if fraction >= criticalLimit { return .orange }
        if fraction >= watchLimit { return .amber }
        return .emerald
    }

    var color: Color {
        guard let pair else { return Palette.textSecondary }
        return Self.color(dark: pair.dark, light: pair.light)
    }

    var pair: (dark: UInt32, light: UInt32)? {
        switch self {
        case .unknown: return nil
        // sRGB conversions of the OKLCH quota tokens in matra-theme.css.
        case .emerald: return (0x59d38c, 0x006132)
        case .amber: return (0xf7b83d, 0x7c5500)
        case .orange: return (0xff8950, 0xa13b00)
        case .red: return (0xff6f69, 0xa50d1c)
        }
    }

    static func stroke(for fraction: Double?, style: ColorTransitionStyle,
                       override: UsageBand? = nil, watchLimit: Double = 0.50,
                       criticalLimit: Double = 0.70) -> Color {
        // Some providers display remaining balance, with a separately computed
        // severity. Do not reinterpret their displayed fraction as used quota.
        if let override {
            switch override {
            case .ample: return emerald.color
            case .watch: return amber.color
            case .critical: return orange.color
            case .exhausted: return red.color
            }
        }
        guard let fraction, fraction.isFinite else { return unknown.color }
        guard style == .ramp else {
            return band(for: fraction, watchLimit: watchLimit, criticalLimit: criticalLimit).color
        }
        let f = min(max(fraction, 0), 1)
        if f >= 0.90 { return red.color }
        if f < watchLimit {
            return Palette.ramp(from: emerald.pair!, to: amber.pair!, fraction: f / watchLimit)
        }
        if f < criticalLimit {
            return Palette.ramp(from: amber.pair!, to: orange.pair!,
                                fraction: (f - watchLimit) / (criticalLimit - watchLimit))
        }
        return Palette.ramp(from: orange.pair!, to: red.pair!,
                            fraction: (f - criticalLimit) / (0.90 - criticalLimit))
    }

    private static func color(dark: UInt32, light: UInt32) -> Color {
        Color(dark: NSColor(hex: dark), light: NSColor(hex: light))
    }
}

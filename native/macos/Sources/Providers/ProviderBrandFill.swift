import AppKit
import SwiftUI

/// How a provider's mark is filled when Settings > Brand colours is on.
///
/// Off, every mark is drawn in the label colour, exactly as before. On, the
/// same traced shapes at the same sizes are filled with the owner-approved
/// brand colours; marks whose brands are black and white stay monochrome.
/// Usage rings and bars never read this: their colours are the quota bands.
enum ProviderBrandFill: Equatable {
    case monochrome
    case solid(UInt32)
    /// The label colour, with one separate part of the mark in a colour.
    /// `region` is in the mark's unit box (top-left origin) and holds that part
    /// only, so it can be painted without touching the rest of the mark.
    case accentPart(UInt32, region: CGRect)
    /// Hard-edged bands from top to bottom, in the order given.
    case bands([UInt32])
    /// The Gemini sparkle: blue right, green bottom, yellow left, red top.
    case googleSpark
    /// The Antigravity arch: red and yellow at the crown, green, blue at the feet.
    case googleArch

    static let googleBlue: UInt32 = 0x4285F4
    static let googleRed: UInt32 = 0xEA4335
    static let googleYellow: UInt32 = 0xFBBC05
    static let googleGreen: UInt32 = 0x34A853

    /// Clockwise from the trailing edge, closing on the colour it opened with.
    static let sparkColors: [UInt32] = [googleBlue, googleGreen, googleYellow, googleRed, googleBlue]
    /// From the crown down to the feet.
    static let archStops: [(color: UInt32, location: CGFloat)] = [
        (googleRed, 0), (googleYellow, 0.30), (googleGreen, 0.55), (googleBlue, 0.85)
    ]

    /// The SwiftUI fill, or nil when the mark keeps the label colour.
    var shapeStyle: AnyShapeStyle? {
        switch self {
        case .monochrome, .accentPart:
            return nil
        case .solid(let hex):
            return AnyShapeStyle(Color(hex: hex))
        case .bands(let colors):
            return AnyShapeStyle(LinearGradient(stops: Self.bandStops(colors).map {
                Gradient.Stop(color: Color(hex: $0.color), location: $0.location)
            }, startPoint: .top, endPoint: .bottom))
        case .googleSpark:
            return AnyShapeStyle(AngularGradient(colors: Self.sparkColors.map { Color(hex: $0) },
                                                 center: .center))
        case .googleArch:
            return AnyShapeStyle(LinearGradient(stops: Self.archStops.map {
                Gradient.Stop(color: Color(hex: $0.color), location: $0.location)
            }, startPoint: .top, endPoint: .bottom))
        }
    }

    /// The sparkle's colour a `fraction` of the way clockwise round from the
    /// trailing edge, blended between its evenly spaced stops as SwiftUI's
    /// angular gradient does.
    static func sparkColor(at fraction: CGFloat) -> CGColor {
        let stops = sparkColors.map { NSColor(hex: $0) }
        let position = min(max(fraction, 0), 1) * CGFloat(stops.count - 1)
        let lower = min(Int(position), stops.count - 2)
        let t = position - CGFloat(lower)
        let a = stops[lower], b = stops[lower + 1]
        return CGColor(srgbRed: a.redComponent + (b.redComponent - a.redComponent) * t,
                       green: a.greenComponent + (b.greenComponent - a.greenComponent) * t,
                       blue: a.blueComponent + (b.blueComponent - a.blueComponent) * t,
                       alpha: 1)
    }

    /// Two stops per band, the second sharing the next band's start, so the
    /// bands meet on a hard edge rather than blending.
    static func bandStops(_ colors: [UInt32]) -> [(color: UInt32, location: CGFloat)] {
        guard !colors.isEmpty else { return [] }
        let step = 1 / CGFloat(colors.count)
        return colors.enumerated().flatMap { index, color in
            [(color, CGFloat(index) * step), (color, CGFloat(index + 1) * step)]
        }
    }

    /// Recolours whatever was just drawn in `rect`: call inside a transparency
    /// layer after drawing the mark in opaque ink. Source-in keeps the mark's
    /// shape and takes the colour's own opacity, so a brand colour lands fully
    /// opaque and `label` (the label colour, translucent) lands exactly as an
    /// ordinary monochrome mark would. `rect` is the mark's box, unflipped.
    func paint(over rect: NSRect, in context: CGContext, label: CGColor) {
        context.saveGState()
        context.setBlendMode(.sourceIn)
        func color(_ hex: UInt32) -> CGColor { NSColor(hex: hex).cgColor }
        switch self {
        case .monochrome:
            context.setFillColor(label)
            context.fill(rect)
        case .solid(let hex):
            context.setFillColor(color(hex))
            context.fill(rect)
        case .accentPart(let hex, let region):
            let part = CGRect(x: rect.minX + region.minX * rect.width,
                              y: rect.maxY - region.maxY * rect.height,
                              width: region.width * rect.width,
                              height: region.height * rect.height)
            context.setFillColor(color(hex))
            context.fill(part)
            context.setFillColor(label)
            context.addRect(rect)
            context.addRect(part)
            context.fillPath(using: .evenOdd)
        case .bands(let colors):
            let height = rect.height / CGFloat(max(colors.count, 1))
            for (index, hex) in colors.enumerated() {
                context.setFillColor(color(hex))
                context.fill(CGRect(x: rect.minX, y: rect.maxY - CGFloat(index + 1) * height,
                                    width: rect.width, height: height))
            }
        case .googleSpark:
            // No conic gradient in this SDK's CoreGraphics, so the sweep is
            // sampled as thin wedges. Angles grow counter-clockwise in an
            // unflipped context while SwiftUI's angular gradient runs
            // clockwise on screen, so each wedge samples the mirrored angle.
            context.clip(to: rect)
            let center = CGPoint(x: rect.midX, y: rect.midY)
            let radius = hypot(rect.width, rect.height)
            let wedges = 72
            for index in 0..<wedges {
                let start = CGFloat(index) / CGFloat(wedges) * 2 * .pi
                let end = CGFloat(index + 1) / CGFloat(wedges) * 2 * .pi
                let middle = (CGFloat(index) + 0.5) / CGFloat(wedges)
                context.setFillColor(Self.sparkColor(at: 1 - middle))
                context.move(to: center)
                // A hair of overlap, so no seam shows between wedges.
                context.addArc(center: center, radius: radius, startAngle: start,
                               endAngle: end + 0.01, clockwise: false)
                context.closePath()
                context.fillPath()
            }
        case .googleArch:
            let colors = Self.archStops.map { color($0.color) } as CFArray
            let locations = Self.archStops.map(\.location)
            if let gradient = CGGradient(colorsSpace: nil, colors: colors, locations: locations) {
                context.clip(to: rect)
                context.drawLinearGradient(gradient, start: CGPoint(x: rect.midX, y: rect.maxY),
                                           end: CGPoint(x: rect.midX, y: rect.minY),
                                           options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
            }
        }
        context.restoreGState()
    }
}

extension ProviderGlyph {
    /// Kimi's dot: the first path of `glyph-kimi`'s SVG sits alone in the top
    /// right corner (x 19.92 to 23.77, y 0 to 3.85 of a 24 box), clear of the
    /// K's arm, which stops at x 18.4. Only the asset separates it; the traced
    /// fallback outline is the K alone.
    static let kimiDotRegion = CGRect(x: 19.5 / 24, y: 0, width: 4.5 / 24, height: 4.2 / 24)

    /// The owner-approved brand fill for this mark (Settings > Brand colours).
    var brandFill: ProviderBrandFill {
        switch self {
        case .claude:      return .solid(0xD97757)
        case .third:       return .solid(0x20808D)
        case .deepseek:    return .solid(0x4D6BFE)
        case .glm:         return .solid(0x3859F8)
        case .meta:        return .solid(0x0081FB)
        case .kiro:        return .solid(0x9046F0)
        case .minimax:     return .solid(0xE83262)
        case .amp:         return .solid(0xF34A3C)
        case .qianwenAI:   return .solid(0xFF6A00)
        case .kimi:        return .accentPart(0x0078F8, region: Self.kimiDotRegion)
        case .mistral:     return .bands([0xF8A800, 0xF85008, 0xE00000])
        case .geminiSpark: return .googleSpark
        case .antigravity: return .googleArch
        case .openai, .cursor, .grok, .copilot, .ollama, .ollamaLocal, .opencode,
             .lmstudio, .kilo, .qwen, .gemma, .devin, .apify, .commandcode:
            return .monochrome
        }
    }

    /// The fill actually used: an accent part is only drawn where the mark
    /// separates that part, which today means the bundled asset.
    func brandFill(drawnFromAsset: Bool) -> ProviderBrandFill {
        if case .accentPart = brandFill, !drawnFromAsset { return .monochrome }
        return brandFill
    }
}

private struct MatraBrandColorsKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Settings > Brand colours. Off by default: marks keep the label colour.
    var matraBrandColors: Bool {
        get { self[MatraBrandColorsKey.self] }
        set { self[MatraBrandColorsKey.self] = newValue }
    }
}

/// A unit-box region of a view, or everything outside it when `inverted`.
struct UnitRegionShape: Shape {
    let region: CGRect
    var inverted = false

    func path(in rect: CGRect) -> Path {
        let inner = CGRect(x: rect.minX + region.minX * rect.width,
                           y: rect.minY + region.minY * rect.height,
                           width: region.width * rect.width,
                           height: region.height * rect.height)
        var path = Path()
        if inverted { path.addRect(rect) }
        path.addRect(inner)
        return path
    }
}

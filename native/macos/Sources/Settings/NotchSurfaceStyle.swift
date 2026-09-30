import SwiftUI

/// Native material choices. Raw values remain stable across display-name changes.
enum NotchSurfaceStyle: String, CaseIterable, Identifiable {
    case system
    case light
    case glass
    case darkGlass
    case solid

    static let allCases: [Self] = [.glass, .darkGlass, .solid, .light, .system]
    static let defaultStyle: Self = .darkGlass
    var id: String { rawValue }

    static var glassAvailable: Bool {
        if #available(macOS 26.0, *) { return true } else { return false }
    }

    var effective: Self { effective(glassAvailable: Self.glassAvailable) }

    /// Both glass themes use light ink and therefore a solid dark fallback.
    func effective(glassAvailable: Bool) -> Self {
        switch self {
        case .glass, .darkGlass: return glassAvailable ? self : .solid
        default: return self
        }
    }

    var isGlass: Bool {
        effective == .glass || effective == .darkGlass
    }

    /// Regular is adaptive Liquid Glass. Clear over a dark backing makes the
    /// deliberately smoked variant without using a tint that can lighten glass.
    @available(macOS 26.0, *)
    var glass: Glass { self == .darkGlass ? .clear : .regular }

    var glassDim: Color? {
        effective == .darkGlass ? Palette.darkGlassDim : nil
    }

    /// Settings uses a native behind-window blur with the same warm material family.
    var settingsWash: Color {
        Palette.glassCharcoal.opacity(self == .darkGlass ? 0.80 : 0.35)
    }

    var title: String {
        switch self {
        case .glass: return "Liquid Glass"
        case .darkGlass: return "Dark Glass"
        case .solid: return "Solid Dark"
        case .light: return "Light"
        case .system: return "System"
        }
    }

    var explanation: String {
        switch self {
        case .glass:
            return Self.glassAvailable
                ? "Adaptive native glass with ivory text. Reduce Transparency uses Solid Dark."
                : "Liquid Glass needs macOS 26 or later. This Mac uses Solid Dark."
        case .darkGlass:
            return Self.glassAvailable
                ? "Smoked charcoal glass with warm DYDXFX accents. Reduce Transparency uses Solid Dark."
                : "Dark Glass needs macOS 26 or later. This Mac uses Solid Dark."
        case .solid: return "Opaque warm near-black with ivory text. No blur or transparency."
        case .light: return "Warm light surfaces throughout Mātrā."
        case .system: return "Solid surfaces follow this Mac's appearance."
        }
    }

    /// Reduce Transparency changes the fill, not the ink palette, for both glass
    /// variants. That keeps text readable while changing themes or accessibility.
    func panelAppearance(reduceTransparency: Bool) -> NSAppearance? {
        switch colorScheme {
        case .light: return NSAppearance(named: .aqua)
        case .dark: return NSAppearance(named: .darkAqua)
        default: return nil
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .light: return .light
        case .glass, .darkGlass, .solid: return .dark
        }
    }
}

private struct NotchSurfaceStyleKey: EnvironmentKey {
    static let defaultValue = NotchSurfaceStyle.defaultStyle
}

extension EnvironmentValues {
    var notchSurfaceStyle: NotchSurfaceStyle {
        get { self[NotchSurfaceStyleKey.self] }
        set { self[NotchSurfaceStyleKey.self] = newValue }
    }
}

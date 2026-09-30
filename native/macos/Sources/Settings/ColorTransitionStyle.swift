import SwiftUI

/// How a usage ring or bar moves from the ample colour to the critical one.
///
/// Raw values are persistence keys, not display copy. The default is `.hardStep` because a
/// visual change to every existing user's notch must be something they opt into, not something
/// that reaches them unannounced the next time the app updates.
enum ColorTransitionStyle: String, CaseIterable, Identifiable {
    /// Discrete status colours at the configured thresholds, plus near exhaustion.
    case hardStep
    /// A continuous blend through the same quota palette, reaching red at 90%.
    case ramp

    var id: String { rawValue }

    var title: String {
        switch self {
        case .hardStep: return L10n.t("Hard step")
        case .ramp: return L10n.t("Colour ramp")
        }
    }

    var explanation: String {
        switch self {
        case .hardStep:
            return "Green below Watch, amber at Watch, orange at Critical, red at 90% used. These status colours stay independent of your accent."
        case .ramp:
            return "Blend from green through amber at Watch and orange at Critical, reaching red at 90% used. Your accent does not change this scale."
        }
    }
}

private struct ColorTransitionStyleKey: EnvironmentKey {
    static let defaultValue = ColorTransitionStyle.hardStep
}

extension EnvironmentValues {
    var colorTransitionStyle: ColorTransitionStyle {
        get { self[ColorTransitionStyleKey.self] }
        set { self[ColorTransitionStyleKey.self] = newValue }
    }
}

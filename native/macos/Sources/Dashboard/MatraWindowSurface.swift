import SwiftUI

/// Settings and Dashboard use one surface recipe and the same Preferences.
/// Native appearance remains owned by the window controller, including System.
struct MatraWindowSurface: ViewModifier {
    @ObservedObject var preferences: Preferences
    @Environment(\.matraReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        content
            .tint(preferences.accentColor.color)
            .environment(\.matraAccentColor, preferences.accentColor.color)
            .environment(\.matraBrandColors, preferences.brandColors)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background {
                if preferences.notchSurfaceStyle.isGlass && !reduceTransparency {
                    VisualEffect(material: .underWindowBackground)
                        .overlay(preferences.notchSurfaceStyle.settingsWash)
                } else {
                    SettingsPalette.window
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: SettingsView.cornerRadius, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: SettingsView.cornerRadius, style: .continuous)
                    .strokeBorder(SettingsPalette.edge, lineWidth: 1)
            }
            .ignoresSafeArea()
    }
}

import SwiftUI

/// Existing dashboard surfaces retain their width and Open details action while
/// sharing the upgraded provider reading rows with the native menu.
struct ProviderTooltip: View {
    var activityNote: String? = nil
    let snapshot: ProviderSnapshot
    let now: Date
    var resetTimeFormat: ResetTimeFormat = .automatic
    var showUsagePace: Bool = false
    var onOpenDetails: (() -> Void)? = nil
    @Environment(\.expandsUsageRows) private var expands
    @State private var width = NotchLayout.cardTextWidth

    var body: some View {
        ProviderLimitsContent(activityNote: activityNote, snapshot: snapshot, now: now,
                              resetTimeFormat: resetTimeFormat, showUsagePace: showUsagePace,
                              headerAccessory: onOpenDetails.map { action in
                                  AnyView(Button("Open details", action: action).buttonStyle(.plain))
                              })
            .environment(\.providerDetailWidth, expands ? width : NotchLayout.cardTextWidth)
            .background(GeometryReader { proxy in
                Color.clear.onAppear { width = max(0, proxy.size.width) }
                    .onChange(of: proxy.size.width) { width = max(0, $0) }
            })
    }
}

extension EnvironmentValues {
    var notchCardSurfaceStyle: NotchSurfaceStyle {
        get { notchSurfaceStyle }
        set { notchSurfaceStyle = newValue }
    }
}

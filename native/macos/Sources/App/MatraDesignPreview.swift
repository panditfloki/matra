import AppKit
import Combine

/// Explicit visual QA mode. Never constructs UsageStore or any provider reader.
@MainActor
final class MatraDesignPreview {
    let settings: SettingsWindowController
    private let preferences: Preferences
    private let fleet: NotchFleet
    private var subscriptions = Set<AnyCancellable>()

    init() {
        let defaults = UserDefaults(suiteName: "com.dydxfx.matra.mac.design-preview")!
        let preferences = Preferences(defaults: defaults)
        self.preferences = preferences
        let fleet = NotchFleet(scope: preferences.notchScope, edge: preferences.notchEdge)
        self.fleet = fleet
        fleet.setSnapshots(Self.samples())
        let settings = SettingsWindowController(
            preferences: preferences, providers: { [] }, updater: Updater(),
            signOut: { _ in }, signIn: { _ in false }, switchAccount: { _ in false }, retry: { _ in },
            resetPosition: { fleet.apply(alongOffset: 0) }, quit: { NSApp.terminate(nil) })
        self.settings = settings
        fleet.onOpenSettings = { [weak settings] in settings?.toggle() }
        fleet.onMoveToEdge = { edge, offset in
            if let offset { preferences.setOffset(offset, for: edge) }
            preferences.notchEdge = edge
        }
        preferences.$notchSurfaceStyle.sink { fleet.apply(surfaceStyle: $0) }.store(in: &subscriptions)
        preferences.$notchEdge.sink { fleet.apply(edge: $0) }.store(in: &subscriptions)
        preferences.$notchVisibility.sink { fleet.apply($0) }.store(in: &subscriptions)
        preferences.$weeklyRing.sink { fleet.apply(weeklyRing: $0) }.store(in: &subscriptions)
        preferences.$weeklyReading.sink { fleet.apply(weeklyReading: $0) }.store(in: &subscriptions)
        preferences.$showsNotchReadings.sink { fleet.apply(showsNotchReadings: $0) }.store(in: &subscriptions)
        preferences.$notchSize.sink { fleet.apply(scale: $0.scale) }.store(in: &subscriptions)
        preferences.$colorTransitionStyle.sink { fleet.apply(colorTransitionStyle: $0) }.store(in: &subscriptions)
    }

    func show() {
        fleet.show()
        settings.show()
    }

    static func samples() -> [ProviderSnapshot] {
        let reset = Date().addingTimeInterval(3600)
        return [
            ProviderSnapshot(id: "claude", displayName: "Claude · Sample", glyph: .claude,
                fidelity: .manual, status: .ok, windows: [
                    LimitWindow(id: "session", label: "Session (sample)", usedFraction: 0.28, resetsAt: reset),
                    LimitWindow(id: "weekly", label: "Weekly (sample)", usedFraction: 0.81, resetsAt: reset)],
                headlineID: "session", weeklyID: "weekly"),
            ProviderSnapshot(id: "codex", displayName: "Codex · Sample", glyph: .openai,
                fidelity: .manual, status: .ok, windows: [
                    LimitWindow(id: "session", label: "Session (sample)", usedFraction: 0.58, resetsAt: reset),
                    LimitWindow(id: "weekly", label: "Weekly (sample)", usedFraction: 0.93, resetsAt: reset)],
                headlineID: "session", weeklyID: "weekly"),
            ProviderSnapshot(id: "gemini", displayName: "Gemini · Sample", glyph: .geminiSpark,
                fidelity: .manual, status: .ok, windows: [
                    LimitWindow(id: "session", label: "Session (sample)", usedFraction: nil, resetsAt: nil)])
        ]
    }
}

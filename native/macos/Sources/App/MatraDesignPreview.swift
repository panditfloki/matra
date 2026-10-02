import AppKit
import Combine

/// Explicit visual QA mode. Never constructs UsageStore or any quota reader.
/// The local-log variant runs only the offline ccusage reader, on open/refresh.
@MainActor
final class MatraDesignPreview {
    let settings: SettingsWindowController
    let dashboard: DashboardWindowController
    let statusItem: StatusItemController
    private let preferences: Preferences
    private let fleet: NotchFleet
    private var subscriptions = Set<AnyCancellable>()

    // Presenters are injectable so regression tests do not mutate the shared
    // WindowServer through real status items or foreground activation.
    init(defaults: UserDefaults = UserDefaults(suiteName: "com.dydxfx.matra.mac.design-preview")!,
         readsLocalLogs: Bool = Runtime.previewReadsLocalLogs,
         localReader: @escaping @Sendable (String) throws -> UsageSpendReport = { try UsageSpendReader.read(providerID: $0) },
         applyPresence: @escaping @MainActor (AppPresence) -> Void = { AppWindowPresentation.restore($0) },
         presentMenuBar: @escaping @MainActor (StatusItemController, Bool) -> Void = { item, visible in
             if visible { item.show() } else { item.hide() }
         }) {
        let preferences = Preferences(defaults: defaults)
        self.preferences = preferences
        let fleet = NotchFleet(scope: preferences.notchScope, edge: preferences.notchEdge)
        self.fleet = fleet
        let samples = readsLocalLogs ? Self.localLogSnapshots() : Self.samples()
        fleet.setSnapshots(samples)
        let settings = SettingsWindowController(
            preferences: preferences, providers: { [] }, updater: Updater(defaults: defaults),
            signOut: { _ in }, signIn: { _ in false }, switchAccount: { _ in false }, retry: { _ in },
            resetPosition: { fleet.apply(alongOffset: 0) }, quit: { NSApp.terminate(nil) })
        self.settings = settings
        let model = readsLocalLogs ? DashboardModel(localLogs: samples, reader: localReader) : DashboardModel(samples: samples)
        let dashboard = DashboardWindowController(model: model,
            preferences: preferences, openSettings: { [weak settings] in settings?.show() })
        self.dashboard = dashboard
        let statusItem = StatusItemController(isPreview: true) { [weak settings] in settings?.show() }
        if readsLocalLogs { statusItem.previewNote = "Local logs only · Quotas not polled" }
        self.statusItem = statusItem
        statusItem.snapshots = samples
        statusItem.onOpenDashboard = { [weak dashboard] in dashboard?.show() }
        statusItem.richMenu = RichMenuBarController(model: dashboard.model, preferences: preferences,
            openSettings: { [weak settings] in settings?.show() },
            openDashboard: { [weak dashboard] id in dashboard?.show(providerID: id) })
        statusItem.onToggleLimits = { [weak preferences] in preferences?.showsLimitsInMenuBar = $0 }
        // Mirror presentation only. No refresh callbacks or provider readers
        // exist in preview mode, including through the menu bar.
        preferences.$appPresence.sink { presence in
            applyPresence(presence)
            presentMenuBar(statusItem, presence.wantsStatusItem)
        }.store(in: &subscriptions)
        Publishers.CombineLatest(preferences.$showsLimitsInMenuBar, preferences.$menuBarProviders)
            .sink { statusItem.limits = MenuBarLimits(isOn: $0, chosen: $1) }.store(in: &subscriptions)
        preferences.$showsWeeklyLimitInMenuBar
            .sink { statusItem.showsWeeklyLimit = $0 }.store(in: &subscriptions)
        preferences.$resetTimeFormat
            .sink { statusItem.resetTimeFormat = $0 }.store(in: &subscriptions)
        fleet.onOpenDashboard = { [weak dashboard] id in dashboard?.show(providerID: id) }
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
        Publishers.CombineLatest(preferences.$watchLimit, preferences.$criticalLimit)
            .sink { fleet.apply(watchLimit: $0, criticalLimit: $1) }.store(in: &subscriptions)
        preferences.$accentColor.sink { fleet.apply(accentColor: $0) }.store(in: &subscriptions)
        preferences.$brandColors.sink { [weak statusItem] in
            fleet.apply(brandColors: $0)
            statusItem?.brandColors = $0
        }.store(in: &subscriptions)
        preferences.$resetTimeFormat.sink { fleet.apply(resetTimeFormat: $0) }.store(in: &subscriptions)
    }

    func show() {
        fleet.show()
        dashboard.show()
    }

    /// Real local logs carry no quota. Each tile says so rather than show a
    /// number; there is no account-wide token history either.
    static func localLogSnapshots() -> [ProviderSnapshot] {
        let sources: [(String, String, ProviderGlyph)] = [("claude", "Claude · Local logs", .claude), ("codex", "Codex · Local logs", .openai)]
        return sources.map { id, name, glyph in
            ProviderSnapshot(id: id, displayName: name, glyph: glyph, fidelity: .derived, status: .ok,
                windows: [LimitWindow(id: "quota", label: "Quota", detail: "Not polled in preview")])
        }
    }

    static func samples() -> [ProviderSnapshot] {
        let reset = Date().addingTimeInterval(3600)
        let calendar = Calendar.current
        let dateFormat = DateFormatter()
        dateFormat.calendar = calendar
        dateFormat.locale = Locale(identifier: "en_US_POSIX")
        dateFormat.dateFormat = "yyyy-MM-dd"
        let buckets = (0..<30).map { offset in
            CodexTokenUsage.DailyBucket(
                startDate: dateFormat.string(from: calendar.date(byAdding: .day, value: offset - 29, to: Date())!),
                tokens: (offset % 7 + 1) * 420_000)
        }
        return [
            ProviderSnapshot(id: "claude", displayName: "Claude · Sample", glyph: .claude,
                fidelity: .manual, status: .ok, windows: [
                    LimitWindow(id: "session", label: "Session (sample)", usedFraction: 0.28, resetsAt: reset, duration: 5 * 3600),
                    LimitWindow(id: "weekly", label: "Weekly (sample)", usedFraction: 0.81,
                        resetsAt: Date().addingTimeInterval(4 * 86400), duration: 7 * 86400)],
                headlineID: "session", weeklyID: "weekly"),
            ProviderSnapshot(id: "codex", displayName: "Codex · Sample", glyph: .openai,
                fidelity: .manual, status: .ok, windows: [
                    LimitWindow(id: "session", label: "Session (sample)", usedFraction: 0.58, resetsAt: reset, duration: 5 * 3600),
                    LimitWindow(id: "weekly", label: "Weekly (sample)", usedFraction: 0.93,
                        resetsAt: Date().addingTimeInterval(4 * 86400), duration: 7 * 86400)],
                headlineID: "session", weeklyID: "weekly",
                tokenUsage: CodexTokenUsage(
                    summary: .init(lifetimeTokens: 120_000_000, peakDailyTokens: 2_940_000,
                                   longestRunningTurnSeconds: 5400, currentStreakDays: 12, longestStreakDays: 18),
                    dailyUsageBuckets: buckets),
                plan: "Sample plan",
                resetCredits: UsageResetCredits(availableCount: 2,
                    credits: [.init(id: "sample-credit", status: "available", expiresAt: reset, count: 2)])),
            ProviderSnapshot(id: "gemini", displayName: "Gemini · Sample", glyph: .geminiSpark,
                fidelity: .manual, status: .ok, windows: [
                    LimitWindow(id: "session", label: "Session (sample)", usedFraction: nil, resetsAt: nil)])
        ]
    }
}

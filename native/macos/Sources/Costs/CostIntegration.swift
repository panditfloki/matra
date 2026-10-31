import AppKit
import Combine
import SwiftUI

/// Where the cost layer plugs into the app: the cost models watch the usage
/// store's snapshots, a Claude card borrows the token chart the Codex card
/// has, and the Activity window opens from the menu.
@MainActor
enum Costs {
    private static var subscriptions: Set<AnyCancellable> = []
    private static var activityWindow: NSWindow?

    /// What a Claude login's card gains before the notch draws it: the daily
    /// token chart and summary the Codex server publishes for Codex, built
    /// here from the transcripts Claude Code writes.
    static func decorate(_ snapshot: ProviderSnapshot) -> ProviderSnapshot {
        // The suite compares cells to what it fed in; the machine's own
        // accounts and transcripts must not leak into that.
        guard !CostRuntime.isInert, snapshot.tokenUsage == nil,
              let usage = CostModels.model(for: snapshot.id)?.tokenUsage else { return snapshot }
        var snapshot = snapshot
        snapshot.tokenUsage = usage
        return snapshot
    }

    static func attach(to store: UsageStore) {
        guard !CostRuntime.isInert else { return }
        subscriptions.removeAll()
        store.$snapshots
            .receive(on: RunLoop.main)
            .sink { snapshots in
                CostAccountStore.shared.rediscover()
                CostModels.all.forEach { $0.observe(snapshots) }
            }
            .store(in: &subscriptions)
        // A new price, rate or billing choice changes money already on screen.
        let reload: () -> Void = { CostModels.all.forEach { $0.reload() } }
        CostAccountStore.shared.$accounts.dropFirst().receive(on: RunLoop.main).sink { _ in reload() }.store(in: &subscriptions)
        PriceTable.shared.$rate.dropFirst().receive(on: RunLoop.main).sink { _ in reload() }.store(in: &subscriptions)
        PriceTable.shared.$prices.dropFirst().receive(on: RunLoop.main).sink { _ in reload() }.store(in: &subscriptions)
        PlanCatalog.shared.$plans.dropFirst().receive(on: RunLoop.main).sink { _ in reload() }.store(in: &subscriptions)
    }

    static func showActivity() {
        if activityWindow == nil {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720),
                             styleMask: [.titled, .closable, .miniaturizable, .resizable],
                             backing: .buffered, defer: false)
            w.title = L10n.t("Activity")
            w.minSize = NSSize(width: 900, height: 560)
            w.contentViewController = NSHostingController(rootView: TimelinePane().frame(minWidth: 900, minHeight: 560))
            w.isReleasedWhenClosed = false
            w.center()
            activityWindow = w
        }
        NSApp.activate(ignoringOtherApps: true)
        activityWindow?.makeKeyAndOrderFront(nil)
    }
}

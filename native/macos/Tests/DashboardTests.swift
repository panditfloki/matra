import AppKit
import SwiftUI
import XCTest
@testable import Matra

@MainActor
final class DashboardTests: XCTestCase {
    private func defaults() throws -> UserDefaults {
        let name = "DashboardTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        addTeardownBlock { defaults.removePersistentDomain(forName: name) }
        return defaults
    }

    func testSelectionSurvivesReadingsButNotRemoval() {
        var samples = MatraDesignPreview.samples()
        let model = DashboardModel(samples: samples)
        model.select("codex")
        samples[1].windows[0] = LimitWindow(id: "session", label: "Session", usedFraction: 0.71)
        model.replaceSnapshots(samples)
        XCTAssertEqual(model.selectedSnapshot?.usedFraction, 0.71)
        model.replaceSnapshots([samples[0]])
        XCTAssertNil(model.selectedID)
        model.select("missing-provider")
        XCTAssertNil(model.selectedSnapshot)
    }

    func testUnknownAndStaleValuesRemainUnchanged() {
        var samples = MatraDesignPreview.samples()
        samples[0].status = .stale(since: Date(timeIntervalSince1970: 100))
        let model = DashboardModel(samples: samples)
        XCTAssertEqual(model.snapshots, samples)
        model.select("gemini")
        XCTAssertNil(model.selectedSnapshot?.usedFraction)
        model.select("claude")
        XCTAssertEqual(model.selectedSnapshot?.status.staleSince, Date(timeIntervalSince1970: 100))
    }

    func testPreviewRefreshCannotCreateLiveReadings() {
        let model = DashboardModel(samples: MatraDesignPreview.samples())
        let before = model.snapshots
        model.refresh()
        XCTAssertTrue(model.isPreview)
        XCTAssertEqual(model.snapshots, before)
        XCTAssertTrue(model.refreshing.isEmpty)
    }

    func testPreviewMenuBarFollowsPresenceAndOffersDashboardWithoutLiveActions() throws {
        let isolatedDefaults = try defaults()
        isolatedDefaults.set(AppPresence.dock.rawValue, forKey: "appPresence")
        // Real status-item creation/activation belongs to native UI acceptance.
        // It otherwise interferes with later offscreen appearance inheritance.
        var presenceChanges: [AppPresence] = []
        var menuVisibility: [Bool] = []
        let preview = MatraDesignPreview(defaults: isolatedDefaults,
                                        applyPresence: { presenceChanges.append($0) },
                                        presentMenuBar: { _, visible in menuVisibility.append(visible) })
        XCTAssertEqual(menuVisibility.last, false)
        preview.dashboard.preferences.appPresence = .menuBar
        XCTAssertEqual(menuVisibility.last, true)
        XCTAssertEqual(presenceChanges.last, .menuBar)
        XCTAssertEqual(preview.statusItem.snapshots, preview.dashboard.model.snapshots)
        XCTAssertNil(preview.statusItem.onRefreshAll)
        XCTAssertNil(preview.statusItem.onRefreshProvider)

        let menu = NSMenu()
        preview.statusItem.rebuild(menu: menu, now: Date())
        XCTAssertEqual(menu.items.first?.title, "Mātrā Preview · Sample data only")
        XCTAssertFalse(menu.items.contains { $0.action == NSSelectorFromString("refreshAll") })
        XCTAssertFalse(menu.items.contains { $0.action == NSSelectorFromString("refreshProvider:") })
        XCTAssertFalse(menu.items.contains { $0.action == NSSelectorFromString("connectPhone") })
        let open = try XCTUnwrap(menu.items.first { $0.title == "Open Dashboard" })
        XCTAssertNotNil(preview.statusItem.onOpenDashboard)
        var opens = 0
        preview.statusItem.onOpenDashboard = { opens += 1 }
        XCTAssertTrue(NSApp.sendAction(try XCTUnwrap(open.action), to: open.target, from: open))
        XCTAssertEqual(opens, 1)
        XCTAssertNil(preview.dashboard.window)
        XCTAssertTrue(preview.dashboard.model.isPreview)

        preview.dashboard.preferences.appPresence = .hidden
        XCTAssertEqual(menuVisibility.last, false)
        XCTAssertEqual(presenceChanges.last, .hidden)
        preview.dashboard.preferences.appPresence = .menuBar
        XCTAssertEqual(menuVisibility.last, true)
        preview.dashboard.preferences.appPresence = .dock
        XCTAssertEqual(menuVisibility.last, false)
        XCTAssertFalse(preview.statusItem.isShowing, "The unit test must never create a real status item")
    }

    func testSharedStoreUpdatesEveryDashboardAndDisconnectClearsSelection() async throws {
        struct Stub: UsageProvider {
            let id = "test"
            let displayName = "Test"
            let glyph = ProviderGlyph.claude
            func fetchSnapshot() async throws -> ProviderSnapshot {
                ProviderSnapshot(id: id, displayName: displayName, glyph: glyph,
                    fidelity: .official, status: .ok,
                    windows: [LimitWindow(id: "session", label: "Session", usedFraction: 0.42)])
            }
        }
        let store = UsageStore(providers: [Stub()], archive: UsageArchive(defaults: try defaults()))
        let first = DashboardModel(store: store)
        let second = DashboardModel(store: store)
        await store.refresh()
        XCTAssertEqual(first.snapshots, store.notchSnapshots)
        XCTAssertEqual(second.snapshots, first.snapshots)
        XCTAssertEqual(first.snapshots.first?.usedFraction, 0.42)
        first.select("test")
        store.disconnected = ["test"]
        XCTAssertTrue(first.snapshots.isEmpty)
        XCTAssertTrue(second.snapshots.isEmpty)
        XCTAssertNil(first.selectedID)
    }

    func testWindowIsReusedAndHasSeparatePreviewFrameKey() throws {
        let controller = DashboardWindowController(model: DashboardModel(samples: []),
            preferences: Preferences(defaults: try defaults()), openSettings: {})
        let first = controller.prepareWindow()
        defer { first.close() }
        XCTAssertTrue(controller.prepareWindow() === first)
        XCTAssertEqual(first.frameAutosaveName, "MatraDashboardPreview")
        XCTAssertEqual(first.title, "Mātrā Dashboard · Sample preview")
    }

    func testRefreshFromTwoSurfacesSharesOneInFlightFetch() async throws {
        actor Counter {
            var calls = 0
            func increment() { calls += 1 }
        }
        struct Stub: UsageProvider {
            let id = "shared"
            let displayName = "Shared"
            let glyph = ProviderGlyph.openai
            let counter: Counter
            func fetchSnapshot() async throws -> ProviderSnapshot {
                await counter.increment()
                return ProviderSnapshot(id: id, displayName: displayName, glyph: glyph,
                                        fidelity: .official, status: .ok, windows: [])
            }
        }
        let counter = Counter()
        let store = UsageStore(providers: [Stub(counter: counter)],
                               archive: UsageArchive(defaults: try defaults()))
        let first = DashboardModel(store: store)
        let second = DashboardModel(store: store)
        first.select("shared")
        second.select("shared")
        first.refresh()
        second.refresh()
        XCTAssertEqual(first.refreshing, ["shared"])
        XCTAssertEqual(second.refreshing, ["shared"])
        await store.refresh(providerID: "shared")?.value
        let calls = await counter.calls
        XCTAssertEqual(calls, 1)
        XCTAssertTrue(first.refreshing.isEmpty)
        XCTAssertTrue(second.refreshing.isEmpty)
    }

    func testAllThemesAndLaterSystemChangesReachTheHostedDashboard() throws {
        let prefs = Preferences(defaults: try defaults())
        let prior = NSApp.appearance
        defer { NSApp.appearance = prior }
        NSApp.appearance = NSAppearance(named: .darkAqua)
        let controller = DashboardWindowController(model: DashboardModel(samples: MatraDesignPreview.samples()),
            preferences: prefs, openSettings: {})
        let window = controller.prepareWindow()
        defer { window.close() }
        let settingsHost = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 860, height: 600),
                                    styleMask: [.titled], backing: .buffered, defer: false)
        settingsHost.isReleasedWhenClosed = false
        let subscription = SettingsWindowController.followAppearance(in: settingsHost, preferences: prefs)
        defer { subscription.cancel(); settingsHost.close() }
        settingsHost.contentView = NSHostingView(rootView: SettingsView(
            preferences: prefs, providers: { [] }, signOut: { _ in }, signIn: { _ in false },
            switchAccount: { _ in false }, retry: { _ in }, resetPosition: {}, quit: {},
            updater: Updater(defaults: try defaults())))

        func assertBoth(_ expected: NSAppearance.Name) {
            for host in [window, settingsHost] {
                host.contentView?.layoutSubtreeIfNeeded()
            }
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            for host in [window, settingsHost] {
                XCTAssertEqual(host.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]), expected)
                XCTAssertEqual(host.contentView?.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]), expected)
            }
        }
        for style in NotchSurfaceStyle.allCases {
            prefs.notchSurfaceStyle = style
            assertBoth(style == .light ? .aqua : .darkAqua)
        }
        prefs.notchSurfaceStyle = .light
        assertBoth(.aqua)
        prefs.notchSurfaceStyle = .system
        XCTAssertNil(window.appearance)
        assertBoth(.darkAqua)
        NSApp.appearance = NSAppearance(named: .aqua)
        assertBoth(.aqua)
        NSApp.appearance = NSAppearance(named: .darkAqua)
        assertBoth(.darkAqua)
    }

    func testThemeAndAccentChangesDoNotResetProviderSelection() throws {
        let prefs = Preferences(defaults: try defaults())
        let model = DashboardModel(samples: MatraDesignPreview.samples())
        let controller = DashboardWindowController(model: model, preferences: prefs, openSettings: {})
        let window = controller.prepareWindow()
        defer { window.close() }
        model.select("codex")
        let snapshots = model.snapshots
        for theme in NotchSurfaceStyle.allCases { prefs.notchSurfaceStyle = theme }
        prefs.accentColor = .blue
        prefs.accentColor = .red
        XCTAssertEqual(model.selectedID, "codex")
        XCTAssertEqual(model.snapshots, snapshots)
    }

    func testClosingOneWindowDoesNotDropPresenceOfAnother() {
        let first = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: false)
        let second = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: false)
        first.isReleasedWhenClosed = false
        second.isReleasedWhenClosed = false
        AppWindowPresentation.register(first)
        AppWindowPresentation.register(second)
        // No foreground order or application policy changes in this test.
        first.setIsVisible(true)
        second.setIsVisible(true)
        defer { first.setIsVisible(false); second.setIsVisible(false); first.close(); second.close() }
        XCTAssertEqual(AppWindowPresentation.policy(for: .hidden, excluding: first), .regular)
        second.setIsVisible(false)
        XCTAssertEqual(AppWindowPresentation.policy(for: .hidden, excluding: first), .accessory)
        XCTAssertEqual(AppWindowPresentation.policy(for: .dock, excluding: first), .regular)
    }

    func testDashboardRendersWithReducedTransparency() throws {
        let prefs = Preferences(defaults: try defaults())
        let model = DashboardModel(samples: MatraDesignPreview.samples())
        for style in [NotchSurfaceStyle.glass, .darkGlass] {
            prefs.notchSurfaceStyle = style
            let view = DashboardView(model: model, preferences: prefs, openSettings: {})
                .environment(\.matraReduceTransparency, true)
                .frame(width: 980, height: 680)
            let image = try XCTUnwrap(ImageRenderer(content: view).nsImage)
            XCTAssertEqual(image.size, NSSize(width: 980, height: 680))
        }
    }

    func testExpandedQuotaTracksUseDashboardWidthWithoutChangingDefault() throws {
        XCTAssertFalse(EnvironmentValues().expandsUsageRows)
        let snapshot = ProviderSnapshot(id: "test", displayName: "Test", glyph: .openai,
            fidelity: .official, status: .ok,
            windows: [LimitWindow(id: "session", label: "Session", usedFraction: 0.50)])
        func render(expanded: Bool) throws -> CGImage {
            let view = ProviderTooltip(snapshot: snapshot, now: Date(timeIntervalSince1970: 1_800_000_000),
                resetTimeFormat: .automatic, showUsagePace: false)
                .environment(\.expandsUsageRows, expanded)
                .environment(\.colorScheme, .dark)
                .frame(width: 600)
            return try XCTUnwrap(ImageRenderer(content: view).cgImage)
        }
        _ = try render(expanded: false)
        let compact = try render(expanded: false)
        let expanded = try render(expanded: true)
        XCTAssertEqual(compact.width, expanded.width)
        XCTAssertEqual(compact.height, expanded.height)
        XCTAssertEqual(compact.bytesPerRow, expanded.bytesPerRow)
        let a = Array(try XCTUnwrap(compact.dataProvider?.data) as Data)
        let b = Array(try XCTUnwrap(expanded.dataProvider?.data) as Data)
        let bytesPerPixel = compact.bitsPerPixel / 8
        var changedBytesAtRight = 0
        for y in 0..<compact.height {
            for x in (compact.width * 2 / 3)..<compact.width {
                let offset = y * compact.bytesPerRow + x * bytesPerPixel
                for channel in 0..<bytesPerPixel where a[offset + channel] != b[offset + channel] {
                    changedBytesAtRight += 1
                }
            }
        }
        XCTAssertGreaterThan(changedBytesAtRight, 100,
                             "The expanded quota track must reach the right side of the dashboard card")
    }
}

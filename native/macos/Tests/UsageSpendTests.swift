import AppKit
import XCTest
@testable import Matra

final class UsageSpendTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_759_392_000) // 2025-10-02 UTC
    private func report(_ json: String, provider: String = "codex") throws -> UsageSpendReport {
        try UsageSpendReport.decodeDaily(Data(json.utf8), providerID: provider, measuredAt: now, timeZoneID: "UTC")
    }

    func testCanonicalTotalsDoNotAddReasoningOrCacheAgain() throws {
        let r = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":120,"inputTokens":30,"outputTokens":20,"reasoningOutputTokens":10,"cacheReadTokens":70,"costUSD":0.02,"models":{"gpt-test":{"totalTokens":120,"reasoningOutputTokens":10}}}]}"#)
        XCTAssertEqual(r.total.tokens, 120)
        XCTAssertEqual(r.models.first?.value.tokens, 120)
        XCTAssertNil(r.models.first?.value.costUSD, "Day-level pricing cannot be assigned to a model")
        XCTAssertEqual(r.total.costUSD, 0.02)
    }

    func testMissingPriceIsUnknownAndPartialTotalsRetainKnownCost() throws {
        let r = try report(#"{"daily":[{"date":"2025-10-01","totalTokens":60,"costUSD":2},{"date":"2025-10-02","totalTokens":120}]}"#)
        XCTAssertEqual(r.total.tokens, 180)
        XCTAssertEqual(r.total.costUSD, 2)
        XCTAssertTrue(r.total.incompleteCost)
        XCTAssertNil(r.today(now: now)?.costUSD)
    }

    func testClaudeUnpricedModelsAreNotFree() throws {
        let r = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":140,"totalCost":3,"modelBreakdowns":[{"modelName":"unknown-new-model","inputTokens":100,"outputTokens":20,"cost":0},{"modelName":"known","inputTokens":20,"cost":3}]}]}"#, provider: "claude")
        XCTAssertEqual(r.total.tokens, 140)
        XCTAssertEqual(r.total.costUSD, 3)
        XCTAssertTrue(r.total.incompleteCost)
        XCTAssertNil(r.models.first { $0.id == "unknown-new-model" }?.value.costUSD)
        let allUnknown = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":100,"totalCost":0,"modelBreakdowns":[{"modelName":"new","inputTokens":100,"cost":0}]}]}"#, provider: "claude")
        XCTAssertNil(allUnknown.total.costUSD)
    }

    func testExplicitEmptyUsagePreservesKnownZero() throws {
        let r = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":0,"costUSD":0}]}"#)
        XCTAssertEqual(r.today(now: now)?.costUSD, 0)
        XCTAssertNil(r.today(now: now.addingTimeInterval(86400)))
    }

    func testFallbackPricingIsVisible() throws {
        let r = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":100,"costUSD":1,"models":{"new":{"totalTokens":100,"isFallback":true}}}]}"#)
        XCTAssertTrue(r.pricingNote.contains("fallback"))
    }

    func testThirtyDayTotalsDoNotBecomeThirtyOneDayMonthTotals() throws {
        let measured = try XCTUnwrap(ISO8601DateFormatter().date(from: "2025-10-31T12:00:00Z"))
        let data = Data(#"{"daily":[{"date":"2025-10-01","totalTokens":10,"costUSD":1},{"date":"2025-10-02","totalTokens":20,"costUSD":2},{"date":"2025-10-31","totalTokens":30,"costUSD":3}]}"#.utf8)
        let r = try UsageSpendReport.decodeDaily(data, providerID: "codex", measuredAt: measured, timeZoneID: "UTC")
        XCTAssertEqual(r.total.tokens, 50)
        XCTAssertEqual(UsageSpendReport.Value.sum(r.days(in: .monthToDate, now: measured).map(\.value)).tokens, 60)
    }

    func testMalformedAndDuplicateDailyRowsAreRejected() {
        for data in [#"{"daily":[{"date":"2025-99-01","totalTokens":3}]}"#,
                     #"{"daily":[{"date":"2025-10-02","totalTokens":-1}]}"#,
                     #"{"daily":[{"date":"2025-10-02","totalTokens":true}]}"#,
                     #"{"daily":[{"date":"2025-10-02","totalTokens":3},{"date":"2025-10-02","totalTokens":3}]}"#] {
            XCTAssertThrowsError(try report(data))
        }
    }

    func testWindowSubtotalExcludesBoundaryDaysAndToday() throws {
        let r = try report(#"{"daily":[{"date":"2025-09-29","totalTokens":10,"costUSD":1},{"date":"2025-09-30","totalTokens":20,"costUSD":2},{"date":"2025-10-01","totalTokens":30,"costUSD":3},{"date":"2025-10-02","totalTokens":40,"costUSD":4}]}"#)
        let start = try XCTUnwrap(r.date(for: "2025-09-29")).addingTimeInterval(3600)
        let end = try XCTUnwrap(r.date(for: "2025-10-03"))
        XCTAssertEqual(r.completeDaySubtotal(start: start, end: end, now: now)?.tokens, 50)
        XCTAssertNil(r.completeDaySubtotal(start: now.addingTimeInterval(-3600), end: end, now: now))
        XCTAssertEqual(r.days(in: .monthToDate, now: now).map(\.id), ["2025-10-01", "2025-10-02"])
    }

    func testProjectAttributionNeverUsesSessionDateDirectory() throws {
        let home = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: home) }
        let data = Data(#"{"sessions":[{"sessionId":"2025/10/02/rollout-019fe5e0-9692-73c0-9873-780bc04bcdf5","directory":"2025/10/02","totalTokens":100,"costUSD":1}]}"#.utf8)
        let rows = try UsageSpendReader.codexProjects(data, home: home)
        XCTAssertEqual(rows.first?.id, "Unattributed")
        XCTAssertEqual(rows.first?.value.tokens, 100)
    }

    func testCommandArgumentsAreNotShellEvaluated() throws {
        let bytes = try UsageSpendReader.run(binary: "/usr/bin/printf", arguments: ["%s", "$(no-command) literal"],
            home: FileManager.default.temporaryDirectory)
        XCTAssertEqual(String(data: bytes, encoding: .utf8), "$(no-command) literal")
    }

    func testFailedReadsBackOffWithoutBlockingManualRefreshOrOtherSources() {
        var policy = UsageSpendRefreshPolicy()
        XCTAssertTrue(policy.claim(providerID: "codex", now: now))
        // No successful report is recorded: the failed attempt alone backs off.
        XCTAssertFalse(policy.claim(providerID: "codex", now: now.addingTimeInterval(149)))
        XCTAssertTrue(policy.claim(providerID: "claude", now: now.addingTimeInterval(1)))
        XCTAssertTrue(policy.claim(providerID: "codex", now: now.addingTimeInterval(150)))
        XCTAssertTrue(policy.claim(providerID: "codex", force: true, now: now.addingTimeInterval(151)))
        XCTAssertFalse(policy.claim(providerID: "codex", now: now.addingTimeInterval(300)))
        XCTAssertTrue(policy.claim(providerID: "codex", now: now.addingTimeInterval(301)))
    }

    // MARK: Cost honesty (DMC-40). Shapes match ccusage 20.0.19; numbers are synthetic.

    func testCodexDayMixingPricedAndUnpricedModelsIsALowerBound() throws {
        // 10-01 proves "old" priced (sole model, cost > 0) and "new" unpriced (cost 0).
        let r = try report(#"{"daily":[{"date":"2025-09-30","totalTokens":50,"costUSD":0,"models":{"new":{"totalTokens":50}}},{"date":"2025-10-01","totalTokens":40,"costUSD":4,"models":{"old":{"totalTokens":40}}},{"date":"2025-10-02","totalTokens":1000,"costUSD":1,"models":{"old":{"totalTokens":100},"new":{"totalTokens":900}}}]}"#)
        let today = try XCTUnwrap(r.today(now: now))
        XCTAssertEqual(today.costUSD, 1)
        XCTAssertTrue(today.incompleteCost)
        XCTAssertEqual(today.pricedTokens, 100)
        XCTAssertEqual(SpendCopy.cost(today), "At least $1.00")
        XCTAssertEqual(SpendCopy.coverage(today), "Cost coverage: 10% of tokens priced. Unpriced tokens add no cost here.")
        let unpricedDay = try XCTUnwrap(r.days.first { $0.id == "2025-09-30" }).value
        XCTAssertNil(unpricedDay.costUSD)
        XCTAssertEqual(SpendCopy.cost(unpricedDay), "Cost unavailable")
        let fullyPriced = try XCTUnwrap(r.days.first { $0.id == "2025-10-01" }).value
        XCTAssertEqual(SpendCopy.cost(fullyPriced), "~$4.00")
        let week = UsageSpendReport.Value.sum(r.days(in: .week, now: now).map(\.value))
        XCTAssertTrue(week.incompleteCost)
        XCTAssertEqual(SpendCopy.cost(week), "At least $5.00")
        XCTAssertEqual(week.pricedTokens, 140)
        XCTAssertEqual(SpendCopy.cost(r.models.first { $0.id == "new" }?.value), "Cost unavailable")
        XCTAssertEqual(SpendCopy.cost(r.models.first { $0.id == "old" }?.value), "Priced, not itemised")
        XCTAssertTrue(r.pricingNote.contains("No offline price for new"))
        XCTAssertFalse(r.pricingNote.contains("old"))
    }

    func testUnprovenCodexModelsNeverCountAsPriced() throws {
        // Two models, one priced day, no way to tell which one has the price.
        let r = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":100,"costUSD":2,"models":{"a":{"totalTokens":60},"b":{"totalTokens":40}}}]}"#)
        let today = try XCTUnwrap(r.today(now: now))
        XCTAssertTrue(today.incompleteCost)
        XCTAssertEqual(today.pricedTokens, 0)
        XCTAssertEqual(SpendCopy.cost(today), "At least $2.00")
        XCTAssertFalse(SpendCopy.cost(today).hasPrefix("~"))
    }

    func testSessionEvidenceProvesSingleModelPricing() throws {
        let sessions = Data(#"{"sessions":[{"sessionId":"x","totalTokens":60,"costUSD":3,"models":{"a":{"totalTokens":60}}},{"sessionId":"y","totalTokens":40,"costUSD":0,"models":{"b":{"totalTokens":40}}}]}"#.utf8)
        let evidence = UsageSpendReport.sessionEvidence(sessions)
        XCTAssertEqual(UsageSpendReport.modelPricing(evidence), ["a": true, "b": false])
        let data = Data(#"{"daily":[{"date":"2025-10-02","totalTokens":100,"costUSD":3,"models":{"a":{"totalTokens":60},"b":{"totalTokens":40}}}]}"#.utf8)
        let r = try UsageSpendReport.decodeDaily(data, providerID: "codex", measuredAt: now, timeZoneID: "UTC", evidence: evidence)
        XCTAssertEqual(r.today(now: now)?.pricedTokens, 60)
        XCTAssertEqual(SpendCopy.coverage(try XCTUnwrap(r.today(now: now))), "Cost coverage: 60% of tokens priced. Unpriced tokens add no cost here.")
    }

    func testPricingInferenceChainsThroughMixedRows() {
        let rows = [UsageSpendReport.PricingEvidence(models: ["a", "b"], costUSD: 2),
                    UsageSpendReport.PricingEvidence(models: ["b"], costUSD: 0),
                    UsageSpendReport.PricingEvidence(models: ["a", "c"], costUSD: 1)]
        XCTAssertEqual(UsageSpendReport.modelPricing(rows), ["a": true, "b": false])
    }

    func testCoverageIsTokenWeightedNotDayCounted() {
        let tiny = UsageSpendReport.Value(tokens: 1000, costUSD: 1, incompleteCost: true, pricedTokens: 3)
        XCTAssertEqual(SpendCopy.coverage(tiny), "Cost coverage: under 1% of tokens priced. Unpriced tokens add no cost here.")
        let full = UsageSpendReport.Value(tokens: 1000, costUSD: 1, pricedTokens: 1000)
        XCTAssertEqual(SpendCopy.coverage(full), "Cost coverage: 100% of tokens priced.")
        XCTAssertEqual(SpendCopy.coverage(.init(tokens: 10, costUSD: nil)), "Cost coverage: 0% of tokens priced. Unpriced tokens add no cost here.")
    }

    func testClaudePartialDayKeepsLowerBoundAndCoverage() throws {
        let r = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":140,"totalCost":3,"modelBreakdowns":[{"modelName":"new","inputTokens":120,"cost":0},{"modelName":"known","inputTokens":20,"cost":3}]}]}"#, provider: "claude")
        let today = try XCTUnwrap(r.today(now: now))
        XCTAssertEqual(SpendCopy.cost(today), "At least $3.00")
        XCTAssertEqual(today.pricedTokens, 20)
        XCTAssertEqual(SpendCopy.cost(r.models.first { $0.id == "known" }?.value), "~$3.00")
    }

    func testRecentWindowCostIsALowerBound() {
        let complete = UsageSpendReport.Value(tokens: 10, costUSD: 2, pricedTokens: 10)
        XCTAssertEqual(SpendCopy.lowerBoundCost(complete), "At least $2.00")
        XCTAssertEqual(SpendCopy.lowerBoundCost(.init(tokens: 10, costUSD: nil)), "Cost unavailable")
    }

    func testCombinedTotalsNameAStaleComponent() throws {
        var codex = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":1,"costUSD":1}]}"#)
        let claude = try report(#"{"daily":[{"date":"2025-10-02","totalTokens":1,"costUSD":1}]}"#, provider: "claude")
        XCTAssertNil(SpendCopy.staleNote([codex, claude]))
        codex.staleReason = "failed"
        XCTAssertEqual(SpendCopy.staleNote([codex, claude]), "Includes an older Codex report: its last refresh failed.")
    }

    func testProjectRowsCountOnlyProvenPricedModels() throws {
        let home = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: home) }
        let data = Data(#"{"sessions":[{"sessionId":"2025/10/02/rollout-019fe5e0-9692-73c0-9873-780bc04bcdf5","totalTokens":100,"costUSD":1,"models":{"old":{"totalTokens":20},"new":{"totalTokens":80}}}]}"#.utf8)
        let row = try XCTUnwrap(UsageSpendReader.codexProjects(data, home: home, pricedModels: ["old"]).first)
        XCTAssertEqual(row.value.pricedTokens, 20)
        XCTAssertEqual(SpendCopy.cost(row.value), "At least $1.00")
    }

    func testInstalledOfflineReaders() throws {
        guard ProcessInfo.processInfo.environment["MATRA_LOCAL_ANALYTICS_ACCEPTANCE"] == "1" else {
            throw XCTSkip("Explicit local analytics acceptance opt-in")
        }
        for provider in ["codex", "claude"] {
            let r = try UsageSpendReader.read(providerID: provider)
            XCTAssertFalse(r.days.isEmpty)
            XCTAssertGreaterThan(r.total.tokens, 0)
            XCTAssertEqual(r.providerID, provider)
            XCTAssertNotNil(r.total.knownPricedTokens, "Coverage must be provable, not inferred from a cost total")
            print("LOCAL ANALYTICS \(provider): \(r.days.count) days, \(r.total.tokens) tokens, \(SpendCopy.cost(r.total)), \(SpendCopy.coverage(r.total)) projects=\(r.projects.count)")
        }
    }
}

@MainActor final class RichMenuBarTests: XCTestCase {
    func testNativeStatusButtonPresentsThePopover() throws {
        guard ProcessInfo.processInfo.environment["MATRA_LOCAL_ANALYTICS_ACCEPTANCE"] == "1" else {
            throw XCTSkip("Explicit native status-button acceptance opt-in")
        }
        let key = "RichMenuNative.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: key))
        defer { defaults.removePersistentDomain(forName: key) }
        let model = DashboardModel(samples: MatraDesignPreview.samples())
        let item = StatusItemController(isPreview: true, onOpenSettings: {})
        item.richMenu = RichMenuBarController(model: model, preferences: Preferences(defaults: defaults),
            openSettings: {}, openDashboard: { _ in })
        item.show()
        defer { item.hide() }
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        item.openUsagePanel()
        RunLoop.main.run(until: Date().addingTimeInterval(0.4))
        XCTAssertTrue(try XCTUnwrap(item.richMenu?.popover).isShown)
        XCTAssertTrue(try XCTUnwrap(item.richMenu?.popover?.contentViewController?.view.window).isVisible)
        // The reading column alone; a detail opens in a panel beside it.
        let content = try XCTUnwrap(item.richMenu?.popover?.contentViewController?.view)
        XCTAssertEqual(content.bounds.width, RichMenuBarController.compactWidth, accuracy: 1)
        XCTAssertGreaterThan(content.bounds.height, 400)
        let menu = try XCTUnwrap(item.richMenu)
        XCTAssertNil(menu.detailPanel, "no panel until a card is hovered or pinned")
        let firstID = try XCTUnwrap(model.snapshots.first?.id)
        menu.hover.activateCard(firstID)
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        let detail = try XCTUnwrap(menu.detailPanel)
        let window = try XCTUnwrap(content.window)
        XCTAssertTrue(detail.isVisible)
        XCTAssertFalse(detail.isKeyWindow, "the panel never takes key focus")
        XCTAssertTrue(detail.parent === window, "it moves and closes with the popover")
        XCTAssertEqual(content.bounds.width, RichMenuBarController.compactWidth, accuracy: 1, "the popover does not grow")
        let popoverFrame = window.convertToScreen(content.convert(content.bounds, to: nil))
        XCTAssertEqual(detail.frame.width, RichMenuBarController.detailWidth, accuracy: 1)
        XCTAssertTrue(abs(detail.frame.minX - popoverFrame.maxX) < 1 || abs(detail.frame.maxX - popoverFrame.minX) < 1,
                      "flush against one side of the popover")
        XCTAssertTrue(try XCTUnwrap(window.screen).visibleFrame.contains(detail.frame))
        print("POPOVER before second action: \(item.richMenu?.popover?.isShown == true)")
        item.openUsagePanel()
        print("POPOVER after second action: \(item.richMenu?.popover?.isShown == true)")
        // Allow AppKit a run-loop turn after the immediate menu dismissal.
        RunLoop.main.run(until: Date().addingTimeInterval(0.4))
        XCTAssertFalse(try XCTUnwrap(item.richMenu?.popover).isShown)
        XCTAssertFalse(detail.isVisible, "the panel closes with the popover")
        XCTAssertNil(menu.hover.state.pinnedID, "and nothing stays pinned for the next opening")
    }

    func testPopoverAndDashboardShareModelAndThemeWithoutPresenting() throws {
        let key = "RichMenuTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: key))
        defer { defaults.removePersistentDomain(forName: key) }
        let preferences = Preferences(defaults: defaults)
        let model = DashboardModel(samples: MatraDesignPreview.samples())
        let panel = RichMenuBarController(model: model, preferences: preferences, openSettings: {}, openDashboard: { _ in })
        let popover = panel.preparePopover()
        XCTAssertTrue(panel.preparePopover() === popover)
        XCTAssertTrue(panel.model === model)
        XCTAssertTrue(panel.preferences === preferences)
        XCTAssertFalse(popover.isShown)
        for style in NotchSurfaceStyle.allCases {
            preferences.notchSurfaceStyle = style
            XCTAssertEqual(popover.appearance?.name, style.panelAppearance(reduceTransparency: false)?.name)
        }
        XCTAssertNil(popover.appearance, "System clears overrides")
        XCTAssertEqual(Set(model.spendReports.keys), ["codex", "claude", "gemini"])
    }
}

/// Live-local preview: real offline analytics, no quota numbers, no store.
@MainActor final class LocalLogPreviewTests: XCTestCase {
    nonisolated private static func fixture(_ id: String) throws -> UsageSpendReport {
        try UsageSpendReport.decodeDaily(Data(#"{"daily":[{"date":"2025-10-02","totalTokens":7,"costUSD":0,"models":{"m":{"totalTokens":7}}}]}"#.utf8),
            providerID: id, measuredAt: Date(timeIntervalSince1970: 1_759_392_000), timeZoneID: "UTC")
    }

    func testLocalLogModeReadsOnlyTheOfflineReaderAndShowsNoQuotaNumbers() throws {
        let snapshots = MatraDesignPreview.localLogSnapshots()
        XCTAssertEqual(snapshots.map(\.id), ["claude", "codex"])
        for snapshot in snapshots {
            XCTAssertTrue(snapshot.windows.allSatisfy { $0.usedFraction == nil && $0.resetsAt == nil })
            XCTAssertEqual(snapshot.windows.first?.detail, "Not polled in preview")
            XCTAssertNil(snapshot.tokenUsage)
        }
        let reads = ReadLog()
        let model = DashboardModel(localLogs: snapshots) { id in reads.add(id); return try Self.fixture(id) }
        XCTAssertTrue(model.isPreview)
        XCTAssertTrue(model.readsLocalLogs)
        XCTAssertTrue(model.canRefresh)
        XCTAssertEqual(model.previewTitle, "Local-log preview")
        XCTAssertTrue(model.spendReports.isEmpty, "No sample reports in local-log mode")
        model.loadAnalytics()
        let deadline = Date().addingTimeInterval(3)
        while model.spendReports.count < 2 && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.02)) }
        XCTAssertEqual(Set(model.spendReports.keys), ["codex", "claude"])
        XCTAssertEqual(SpendCopy.cost(model.spendReports["codex"]?.total), "Cost unavailable")
        // A store-like publication must not start background reads here.
        model.replaceSnapshots(snapshots)
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        XCTAssertEqual(reads.ids.sorted(), ["claude", "codex"])
    }

    func testSampleModeNeverRunsTheReader() {
        let model = DashboardModel(samples: MatraDesignPreview.samples())
        XCTAssertFalse(model.canRefresh)
        XCTAssertEqual(model.previewTitle, "Sample preview")
        model.loadAnalytics(force: true)
        XCTAssertTrue(model.analyticsLoading.isEmpty)
    }

    func testPreviewHarnessWiresLocalLogsWithoutRefreshCallbacks() throws {
        let name = "LocalLogPreviewTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let preview = MatraDesignPreview(defaults: defaults, readsLocalLogs: true,
            localReader: { try Self.fixture($0) }, applyPresence: { _ in }, presentMenuBar: { _, _ in })
        XCTAssertTrue(preview.dashboard.model.readsLocalLogs)
        XCTAssertEqual(preview.statusItem.snapshots, MatraDesignPreview.localLogSnapshots())
        XCTAssertNil(preview.statusItem.onRefreshAll)
        XCTAssertNil(preview.statusItem.onRefreshProvider)
        let menu = NSMenu()
        preview.statusItem.rebuild(menu: menu, now: Date())
        XCTAssertEqual(menu.items.first?.title, "Mātrā Preview · Local logs only · Quotas not polled")
    }

    func testLivePreviewNeedsTheExplicitPreviewFlag() {
        XCTAssertTrue(Runtime.usesLivePreview(environment: ["MATRA_DESIGN_PREVIEW": "1", "MATRA_PREVIEW_DATA": "live"]))
        XCTAssertFalse(Runtime.usesLivePreview(environment: ["MATRA_PREVIEW_DATA": "live"]))
        XCTAssertFalse(Runtime.usesLocalLogPreview(environment: ["MATRA_DESIGN_PREVIEW": "1", "MATRA_PREVIEW_DATA": "live"]))
    }

    func testOnlyThePreviewIdentityGetsSeparateStorage() {
        // The test host carries the installed identity, so storage is unchanged here.
        XCTAssertFalse(MatraStorage.isPreviewIdentity)
        XCTAssertEqual(MatraStorage.namespace, "com.dydxfx.matra.mac")
        XCTAssertEqual(MatraStorage.supportRoot(in: URL(fileURLWithPath: "/x")).lastPathComponent, "Matra")
        XCTAssertNotEqual(MatraStorage.previewBundleID, MatraStorage.installedBundleID)
    }

    func testLocalLogModeNeedsTheExplicitPreviewFlag() {
        XCTAssertTrue(Runtime.usesLocalLogPreview(environment: ["MATRA_DESIGN_PREVIEW": "1", "MATRA_PREVIEW_DATA": "local"]))
        XCTAssertFalse(Runtime.usesLocalLogPreview(environment: ["MATRA_PREVIEW_DATA": "local"]))
        XCTAssertFalse(Runtime.usesLocalLogPreview(environment: ["MATRA_DESIGN_PREVIEW": "1"]))
    }
}

private final class ReadLog: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String] = []
    func add(_ id: String) { lock.lock(); values.append(id); lock.unlock() }
    var ids: [String] { lock.lock(); defer { lock.unlock() }; return values }
}

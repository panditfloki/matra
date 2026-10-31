import XCTest
@testable import Matra

final class UsageFreshnessTests: XCTestCase {
    /// Answers anything, and remembers what it was asked for.
    private final class RecordingProvider: UsageProvider, @unchecked Sendable {
        let id = "recorder"
        let displayName = "Recorder"
        let glyph = ProviderGlyph.claude
        private(set) var asked: [UsageFreshness] = []

        func fetchSnapshot() async throws -> ProviderSnapshot {
            try await fetchSnapshot(freshness: .standard)
        }

        func fetchSnapshot(freshness: UsageFreshness) async throws -> ProviderSnapshot {
            asked.append(freshness)
            return ProviderSnapshot(id: id, displayName: displayName, glyph: glyph,
                                    fidelity: .official, status: .ok,
                                    windows: [LimitWindow(id: "session", label: "Current session",
                                                          usedFraction: 0.1)])
        }
        nonisolated func account() -> ProviderAccount? { nil }
        nonisolated var signInRoute: SignInRoute { .guidance("") }
        func signOut() async {}
        func presentSignIn() {}
        nonisolated func forgetCachedCredential() {}
    }

    private func makeDefaults() -> UserDefaults {
        let name = "UsageFreshnessTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return defaults
    }

    @MainActor
    private func store(_ provider: RecordingProvider) -> UsageStore {
        UsageStore(providers: [provider], archive: UsageArchive(defaults: makeDefaults()))
    }

    /// Nothing is running, so nothing has moved, so a provider's own cache is
    /// the best answer available — free, and not wrong about a still number.
    @MainActor
    func testAnIdleRefreshTakesWhateverTheProviderHasCached() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { false }

        await store.refresh()

        XCTAssertEqual(provider.asked, [.standard])
    }

    /// Something is running, so the cached number is the one thing it cannot be:
    /// current.
    @MainActor
    func testARefreshWhileBusyWillNotTakeACachedReading() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { true }

        await store.refresh()

        XCTAssertEqual(provider.asked, [.live])
    }

    /// A look is the least excusable moment to answer from a cache, whether or
    /// not anything is running.
    @MainActor
    func testALookAsksForALiveReading() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { false }

        store.refreshBecauseSomeoneIsLooking()
        await store.settleForTesting()

        XCTAssertEqual(provider.asked, [.live])
    }

    /// And a second look a moment later is answered by the first one's fetch.
    @MainActor
    func testASecondLookInsideTheSpacingAsksForNothing() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { false }

        store.refreshBecauseSomeoneIsLooking()
        await store.settleForTesting()
        store.refreshBecauseSomeoneIsLooking()
        await store.settleForTesting()

        XCTAssertEqual(provider.asked.count, 1, "hovering twice cost two fetches")
    }

    /// Work stopping is the falling edge the idle schedule is about to sit on
    /// for five minutes. It gets one fetch, and it is a live one.
    @MainActor
    func testWorkFinishingAsksForALiveReadingOnce() async {
        let provider = RecordingProvider()
        let store = store(provider)

        store.refreshBecauseWorkFinished(providerID: provider.id)
        await store.settleForTesting()
        // A transcript-read session state legitimately flickers; the second edge
        // inside the spacing must not cost a second fetch.
        store.refreshBecauseWorkFinished(providerID: provider.id)
        await store.settleForTesting()

        XCTAssertEqual(provider.asked, [.live])
    }

    /// Refetching one provider is an event, not a schedule, so it asks for now.
    @MainActor
    func testRefreshingOneProviderAsksForALiveReading() async {
        let provider = RecordingProvider()
        let store = store(provider)

        await store.refresh(providerID: provider.id)?.value

        XCTAssertEqual(provider.asked, [.live])
    }

    /// And a caller that is somebody's own click — Refresh now, a ring clicked,
    /// the settings row's refresh — says so, and is answered from nothing held.
    @MainActor
    func testAClickAsksTheSourceItself() async {
        let provider = RecordingProvider()
        let store = store(provider)

        await store.refresh(providerID: provider.id, freshness: .fromSource)?.value

        XCTAssertEqual(provider.asked, [.fromSource])
    }

    // MARK: - Ask the provider every time you look

    /// The setting's whole purpose: a look stops accepting anything a provider
    /// is holding, however new, and asks the provider.
    @MainActor
    func testTheSettingMakesALookAskTheSourceItself() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { false }
        store.asksProviderOnLook = { true }

        store.refreshBecauseSomeoneIsLooking()
        await store.settleForTesting()

        XCTAssertEqual(provider.asked, [.fromSource])
    }

    /// Off — the shipped default — a look still asks for a live reading, which a
    /// cache newer than a couple of minutes may still answer.
    @MainActor
    func testWithoutTheSettingALookStillAsksForALiveReading() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { false }

        store.refreshBecauseSomeoneIsLooking()
        await store.settleForTesting()

        XCTAssertEqual(provider.asked, [.live])
    }

    /// It reaches the look and nothing else. A setting that also applied to the
    /// schedule would spend an uncached request every thirty seconds through a
    /// long session, at a provider that answers a 429 — which is how asking for
    /// fresher numbers ends up producing staler ones.
    @MainActor
    func testTheSettingDoesNotReachTheSchedule() async {
        let provider = RecordingProvider()
        let store = store(provider)
        store.isBusy = { true }
        store.asksProviderOnLook = { true }

        await store.refresh()

        XCTAssertEqual(provider.asked, [.live])
    }

    /// The switch is off until somebody turns it on, and stays where it is put.
    @MainActor
    func testTheSettingIsOffByDefaultAndPersists() throws {
        let name = "UsageFreshnessTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }

        let preferences = Preferences(defaults: defaults)
        XCTAssertFalse(preferences.asksProviderOnLook)
        preferences.asksProviderOnLook = true
        XCTAssertTrue(Preferences(defaults: defaults).asksProviderOnLook)
    }
}

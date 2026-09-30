import Sparkle
import XCTest
@testable import Matra

@MainActor
final class AutomaticUpdateTests: XCTestCase {
    private func isolatedUpdater(_ body: (Updater) throws -> Void) rethrows {
        let suite = "AutomaticUpdateTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        try body(Updater(defaults: defaults))
    }

    private func item() throws -> SUAppcastItem {
        try XCTUnwrap(SUAppcastItem(dictionary: [
            "enclosure": ["url": "https://example.test/Matra.zip", "sparkle:version": "999"],
            "sparkle:shortVersionString": "9.9.9"
        ]))
    }

    func testAutomaticOfferDownloadsAndInstallsOnlyWhileOptedIn() throws {
        try isolatedUpdater { updater in
            var downloads = 0
            var installs = 0
            updater.automaticallyInstallsUpdates = true
            updater.offer(try item()) { if $0 == .install { downloads += 1 } }
            XCTAssertEqual(downloads, 1)
            XCTAssertEqual(updater.prompt?.phase, .downloading(nil))
            updater.readyToInstall { if $0 == .install { installs += 1 } }
            XCTAssertEqual(installs, 1)
            XCTAssertEqual(updater.prompt?.phase, .installing)
        }
    }

    func testSwitchingOffAfterAutomaticAcceptanceStopsUnattendedRestart() throws {
        try isolatedUpdater { updater in
            var downloads = 0
            var installs = 0
            updater.offer(try item()) { if $0 == .install { downloads += 1 } }
            XCTAssertEqual(downloads, 0)
            XCTAssertEqual(updater.prompt?.phase, .available)
            // Turning On accepts the pending offer as automatic, not manual.
            updater.automaticallyInstallsUpdates = true
            XCTAssertEqual(downloads, 1)
            updater.automaticallyInstallsUpdates = false
            updater.readyToInstall { if $0 == .install { installs += 1 } }
            XCTAssertEqual(installs, 0)
            XCTAssertEqual(updater.prompt?.phase, .available)
            updater.respond(.install)
            XCTAssertEqual(installs, 1, "A later manual acceptance is still allowed")
        }
    }

    func testManualAcceptanceRetainsExplicitConsent() throws {
        try isolatedUpdater { updater in
            var downloads = 0
            var installs = 0
            updater.offer(try item()) { if $0 == .install { downloads += 1 } }
            updater.respond(.install)
            XCTAssertEqual(downloads, 1)
            updater.readyToInstall { if $0 == .install { installs += 1 } }
            XCTAssertEqual(installs, 1)
        }
    }
}

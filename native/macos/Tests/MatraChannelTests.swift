import Foundation
import XCTest
@testable import Matra

final class MatraChannelTests: XCTestCase {
    func testAllIdentitiesHaveSeparateStateAndServices() {
        let channels = [MatraStorage.installedBundleID, MatraStorage.previewBundleID,
                        MatraStorage.developmentBundleID, "invalid.identity"].map { MatraChannel(bundleID: $0) }
        XCTAssertEqual(Set(channels.map(\.namespace)).count, channels.count)
        XCTAssertEqual(Set(channels.map(\.supportFolder)).count, channels.count)
        XCTAssertEqual(Set(channels.map(\.pluginStem)).count, channels.count)
        XCTAssertEqual(Set(channels.map(\.relayPort)).count, channels.count)
        XCTAssertEqual(Set(channels.map(\.phonePort)).count, channels.count)
        XCTAssertEqual(channels.map(\.isProduction), [true, false, false, false])
        XCTAssertEqual(MatraChannel(bundleID: nil), channels.last)
    }

    func testOnlyOrdinaryProductionCanReadUpdateFeed() {
        for id in [MatraStorage.installedBundleID, MatraStorage.previewBundleID,
                   MatraStorage.developmentBundleID, "unknown"] {
            let channel = MatraChannel(bundleID: id)
            XCTAssertEqual(channel.allowsUpdates(underTest: false, sample: false), id == MatraStorage.installedBundleID)
            XCTAssertFalse(channel.allowsUpdates(underTest: true, sample: false))
            XCTAssertFalse(channel.allowsUpdates(underTest: false, sample: true))
        }
    }

    @MainActor func testManualDevelopmentCheckDoesNotStartSparkle() {
        let updater = Updater()
        updater.checkNow()
        XCTAssertFalse(Updater.isEnabled)
        XCTAssertNil(updater.lastChecked)
        guard case .failed = updater.outcome else { return XCTFail("An unavailable update channel must explain why") }
    }

    func testHostedSampleCannotInstallExternalHooks() {
        XCTAssertFalse(Runtime.allowsExternalIntegrationChanges)
        XCTAssertThrowsError(try OpenCodePluginInstaller.install())
        XCTAssertThrowsError(try OpenCodePluginInstaller.uninstall())
    }
}

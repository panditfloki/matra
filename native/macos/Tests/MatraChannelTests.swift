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

    /// The Ollama instruction must name the relay this identity actually runs:
    /// a development build pointing users at 11435 would feed the installed app.
    @MainActor func testOllamaRelayInstructionNamesThisIdentitysPort() {
        let production = MatraChannel(bundleID: MatraStorage.installedBundleID).relayAddress
        let development = MatraChannel(bundleID: MatraStorage.developmentBundleID).relayAddress
        XCTAssertEqual(production, "http://127.0.0.1:11435")
        XCTAssertEqual(development, "http://127.0.0.1:11436")

        let english = OllamaSettingsRow.relayInstruction(address: development, locale: Locale(identifier: "en"))
        XCTAssertEqual(english, "To measure responses, point your chat client to http://127.0.0.1:11436 and keep Mātrā open.")

        // Translated, not the English fallback: the catalog key matches the
        // interpolated source, and each language receives the address.
        for identifier in ["de", "ja", "zh-Hans", "tr"] {
            let text = OllamaSettingsRow.relayInstruction(address: development, locale: Locale(identifier: identifier))
            XCTAssertTrue(text.contains(development), "\(identifier): \(text)")
            XCTAssertFalse(text.contains("11435"), "\(identifier): \(text)")
            XCTAssertFalse(text.hasPrefix("To measure responses"), "\(identifier) fell back to English: \(text)")
        }
        XCTAssertTrue(OllamaSettingsRow.relayInstruction(address: production, locale: Locale(identifier: "de"))
            .contains(production))
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

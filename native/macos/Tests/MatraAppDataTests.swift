import AppKit
import SwiftUI
import XCTest
@testable import Matra

final class MatraAppDataTests: XCTestCase {
    @MainActor func testAppDataCopyIsBrandedAndAccurateForMac() {
        let copy = [MatraAppDataView.tagline, MatraAppDataView.formula,
                    MatraAppDataView.startupHelp, MatraAppDataView.automaticInstallHelp,
                    MatraAppDataView.updateUnavailable,
                    MatraAppDataView.updateHelp, MatraAppDataView.folderHelp].joined(separator: " ")
        for removed in ["Windows", "latest stable release", "Automatic installation is not available"] {
            XCTAssertFalse(copy.localizedCaseInsensitiveContains(removed), removed)
        }
        XCTAssertTrue(copy.contains("not configured"))
        XCTAssertTrue(copy.contains("Install verified Mātrā updates"))
        XCTAssertTrue(copy.contains("separately by macOS"))
        XCTAssertEqual(MatraAppDataView.formula, "Used / limit × 100 = usage %")
    }

    func testOpenFolderPreparationIsScopedAndIdempotent() throws {
        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: temporary) }
        let first = try MatraStorage.prepareSupportRoot(in: temporary)
        let second = try MatraStorage.prepareSupportRoot(in: temporary)
        XCTAssertEqual(first, second)
        XCTAssertEqual(first.lastPathComponent, "Matra")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: temporary.path), ["Matra"])
    }

    func testOpenFolderPreparationPropagatesFileSystemErrors() throws {
        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: temporary) }
        try Data("not a directory".utf8).write(to: temporary)
        XCTAssertThrowsError(try MatraStorage.prepareSupportRoot(in: temporary))
    }

    func testMatraLicenseIsBundled() throws {
        let notice = try XCTUnwrap(Bundle.main.url(forResource: "Matra-LICENSE", withExtension: "txt"))
        let content = try String(contentsOf: notice, encoding: .utf8)
        XCTAssertTrue(content.contains("Copyright (c) 2026 dydxfx"))
        XCTAssertTrue(content.contains("The above copyright notice and this permission notice shall be included"))
        let sourceRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
        XCTAssertEqual(content, try String(contentsOf: sourceRoot.appendingPathComponent("LICENSE"), encoding: .utf8))
    }

    @MainActor func testCurrentReleaseDescribesLiveStartup() throws {
        let note = try XCTUnwrap(ReleaseNotes.all.first)
        XCTAssertEqual(note.version, Updater().currentVersion)
        let details = note.changes.map(\.detail).joined(separator: " ")
        XCTAssertTrue(details.contains("Normal launch reads"))
        XCTAssertFalse(details.contains("sample data by default"))
        XCTAssertTrue(details.contains("automatic installation"))
    }

    func testGlassRingCentresInheritMaterialAndAccessibilityStaysOpaque() {
        for style in NotchSurfaceStyle.allCases {
            for available in [true, false] {
                for reduce in [true, false] {
                    let color = NSColor(ProviderRing.centerFill(for: style,
                        reduceTransparency: reduce, glassAvailable: available))
                    let shouldBeClear = available && !reduce && (style == .glass || style == .darkGlass)
                    XCTAssertEqual(color.alphaComponent, shouldBeClear ? 0 : 1, accuracy: 0.001,
                                   "\(style), glass=\(available), reduce=\(reduce)")
                }
            }
        }
    }
}

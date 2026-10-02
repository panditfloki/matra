import SwiftUI
import XCTest
@testable import Matra

/// Settings > Brand colours: off by default, persisted, and filling each
/// provider mark with exactly the owner-approved colours.
@MainActor
final class BrandColorsTests: XCTestCase {
    private func defaults(_ label: String) -> (UserDefaults, String) {
        let name = "BrandColorsTests.\(label).\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return (defaults, name)
    }

    func testBrandColoursAreOffByDefault() {
        let (defaults, name) = defaults("fresh")
        defer { defaults.removePersistentDomain(forName: name) }
        XCTAssertFalse(Preferences(defaults: defaults).brandColors)
        XCTAssertFalse(EnvironmentValues().matraBrandColors, "views not told otherwise draw monochrome")
    }

    /// Settings written by a build that never had this switch decode as before,
    /// with the switch off.
    func testLegacySettingsDecodeWithBrandColoursOff() {
        let (defaults, name) = defaults("legacy")
        defer { defaults.removePersistentDomain(forName: name) }
        // The accent's stored value is its hex key, not its case name.
        defaults.set(AccentColorChoice.pink.rawValue, forKey: "accentColor")
        defaults.set(true, forKey: "weeklyRingDashed")
        let preferences = Preferences(defaults: defaults)
        XCTAssertFalse(preferences.brandColors)
        XCTAssertEqual(preferences.accentColor, .pink)
        XCTAssertTrue(preferences.weeklyRingDashed)
    }

    func testBrandColoursSurviveARestart() {
        let (defaults, name) = defaults("restart")
        defer { defaults.removePersistentDomain(forName: name) }
        Preferences(defaults: defaults).brandColors = true
        XCTAssertTrue(Preferences(defaults: defaults).brandColors)
        Preferences(defaults: defaults).brandColors = false
        XCTAssertFalse(Preferences(defaults: defaults).brandColors)
    }

    func testColourMapMatchesTheApprovedColours() {
        let expected: [ProviderGlyph: ProviderBrandFill] = [
            .claude: .solid(0xD97757),
            .third: .solid(0x20808D),
            .deepseek: .solid(0x4D6BFE),
            .glm: .solid(0x3859F8),
            .meta: .solid(0x0081FB),
            .kiro: .solid(0x9046F0),
            .minimax: .solid(0xE83262),
            .amp: .solid(0xF34A3C),
            .qianwenAI: .solid(0xFF6A00),
            .kimi: .accentPart(0x0078F8, region: ProviderGlyph.kimiDotRegion),
            .mistral: .bands([0xF8A800, 0xF85008, 0xE00000]),
            .geminiSpark: .googleSpark,
            .antigravity: .googleArch,
        ]
        for (glyph, fill) in expected {
            XCTAssertEqual(glyph.brandFill, fill, "\(glyph)")
        }
    }

    func testBlackAndWhiteBrandsStayMonochrome() {
        let monochrome: [ProviderGlyph] = [
            .openai, .cursor, .grok, .copilot, .ollama, .ollamaLocal, .opencode,
            .lmstudio, .kilo, .qwen, .gemma, .devin, .apify, .commandcode,
        ]
        for glyph in monochrome {
            XCTAssertEqual(glyph.brandFill, .monochrome, "\(glyph)")
            XCTAssertNil(glyph.brandFill.shapeStyle, "\(glyph)")
        }
    }

    func testGoogleMarksUseTheFourGoogleColours() {
        let google: Set<UInt32> = [0x4285F4, 0xEA4335, 0xFBBC05, 0x34A853]
        XCTAssertEqual(Set(ProviderBrandFill.sparkColors), google)
        XCTAssertEqual(Set(ProviderBrandFill.archStops.map(\.color)), google)
        XCTAssertEqual(ProviderBrandFill.sparkColors.first, ProviderBrandFill.sparkColors.last,
                       "the sparkle's sweep closes without a seam")
    }

    /// Kimi's dot is a separate path only in the bundled asset; the traced
    /// fallback is the K alone, so it keeps the label colour throughout.
    func testKimiDotIsColouredOnlyWhereTheMarkSeparatesIt() {
        XCTAssertEqual(ProviderGlyph.kimi.brandFill(drawnFromAsset: true), ProviderGlyph.kimi.brandFill)
        XCTAssertEqual(ProviderGlyph.kimi.brandFill(drawnFromAsset: false), .monochrome)
        XCTAssertEqual(ProviderGlyph.claude.brandFill(drawnFromAsset: false), .solid(0xD97757))
        let dot = ProviderGlyph.kimiDotRegion
        XCTAssertGreaterThan(dot.minX, 18.4 / 24, "clear of the K's upper arm")
        XCTAssertLessThanOrEqual(dot.maxX, 1)
        XCTAssertGreaterThanOrEqual(dot.maxY, 3.846 / 24, "holds the whole dot")
    }

    func testMistralBandsMeetOnHardEdgesTopToBottom() {
        let stops = ProviderBrandFill.bandStops([0xF8A800, 0xF85008, 0xE00000])
        let expectedColors: [UInt32] = [0xF8A800, 0xF8A800, 0xF85008, 0xF85008, 0xE00000, 0xE00000]
        let expectedLocations: [CGFloat] = [0, 1.0 / 3, 1.0 / 3, 2.0 / 3, 2.0 / 3, 1]
        XCTAssertEqual(stops.map(\.color), expectedColors)
        for (actual, expected) in zip(stops.map(\.location), expectedLocations) {
            XCTAssertEqual(actual, expected, accuracy: 1e-9)
        }
    }
}

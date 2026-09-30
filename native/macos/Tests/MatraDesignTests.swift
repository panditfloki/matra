import AppKit
import SwiftUI
import XCTest
@testable import Matra

final class MatraDesignTests: XCTestCase {
    func testQuotaBoundariesAndUnknowns() {
        let readings: [(Double?, MatraQuotaBand)] = [
            (nil, .unknown), (.nan, .unknown), (.infinity, .unknown),
            (-0.1, .emerald), (0, .emerald), (0.4999, .emerald),
            (0.50, .amber), (0.6999, .amber), (0.70, .orange),
            (0.8999, .orange), (0.90, .red), (1, .red), (1.4, .red)
        ]
        for (value, expected) in readings {
            XCTAssertEqual(MatraQuotaBand.band(for: value), expected)
        }
        XCTAssertNotEqual(MatraQuotaBand.band(for: 0.21), MatraQuotaBand.band(for: 0.93))
    }

    func testCustomQuotaThresholdsAndNearLimitSafety() {
        let readings: [(Double, MatraQuotaBand)] = [
            (0.2999, .emerald), (0.30, .amber), (0.5999, .amber),
            (0.60, .orange), (0.8999, .orange), (0.90, .red), (1, .red)
        ]
        for (value, expected) in readings {
            XCTAssertEqual(MatraQuotaBand.band(for: value, watchLimit: 0.30, criticalLimit: 0.60), expected)
        }
        XCTAssertEqual(MatraQuotaBand.band(for: 0.91, watchLimit: 0.95, criticalLimit: 0.99), .red,
                       "A late alert threshold must not hide near-exhausted usage")
    }

    @MainActor func testActualRingsAndBarsIgnoreAccentAndFollowThresholds() throws {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let snapshot = ProviderSnapshot(
            id: "claude", displayName: "Sample", glyph: .claude, fidelity: .official,
            status: .ok, windows: [
                LimitWindow(id: "session", label: "Session", usedFraction: 0.40),
                LimitWindow(id: "weekly", label: "Weekly", usedFraction: 0.65),
                LimitWindow(id: "money", label: "Balance", money: UsageMoneyBreakdown(currency: "USD", spent: 40, remaining: 60))
            ])
        func pixels(accent: Color, scheme: ColorScheme, style: ColorTransitionStyle,
                    watch: Double = 0.50, critical: Double = 0.70) throws -> Data {
            let content = HStack {
                ProviderRing(usedFraction: 0.40, glyph: .claude, weeklyFraction: 0.65, weeklyRing: .outside)
                TooltipCard(snapshot: snapshot, now: now, direction: .trailing)
            }
            .padding(20)
            .environment(\.colorScheme, scheme)
            .environment(\.notchSurfaceStyle, .solid)
            .environment(\.matraHeadlessGlass, true)
            .environment(\.matraAccentColor, accent)
            .environment(\.colorTransitionStyle, style)
            .environment(\.usageWatchLimit, watch)
            .environment(\.usageCriticalLimit, critical)
            let renderer = ImageRenderer(content: content)
            let image = try XCTUnwrap(renderer.cgImage)
            return try XCTUnwrap(image.dataProvider?.data) as Data
        }
        for scheme in [ColorScheme.light, .dark] {
            for style in ColorTransitionStyle.allCases {
                // Prime the renderer after earlier tests have changed drawing
                // state. A same-accent control found eight differing bytes on
                // the first full-suite capture, before any accent change.
                _ = try pixels(accent: .blue, scheme: scheme, style: style)
                let blue = try pixels(accent: .blue, scheme: scheme, style: style)
                let sameBlue = try pixels(accent: .blue, scheme: scheme, style: style)
                let red = try pixels(accent: .red, scheme: scheme, style: style)
                XCTAssertEqual(blue, sameBlue, "\(scheme)/\(style): renderer did not settle for identical inputs")
                XCTAssertEqual(blue, red, "\(scheme)/\(style): app accent changed usage pixels")
                let custom = try pixels(accent: .red, scheme: scheme, style: style, watch: 0.30, critical: 0.60)
                XCTAssertNotEqual(red, custom, "Thresholds must actually reach the rendered rings and bars")
            }
        }
    }

    func testRingAndBarPalettePreservesExplicitBands() {
        // Dynamic AppKit colours have unique provider identities. Compare the
        // resolved pixels in both appearances, not the providers' UUIDs.
        func assertColor(_ actual: Color, matches expected: Color, file: StaticString = #filePath, line: UInt = #line) {
            for appearance in [NSAppearance.Name.aqua, .darkAqua] {
                NSAppearance(named: appearance)!.performAsCurrentDrawingAppearance {
                    guard let a = NSColor(actual).usingColorSpace(.sRGB),
                          let e = NSColor(expected).usingColorSpace(.sRGB) else {
                        return XCTFail("Colour must resolve to sRGB", file: file, line: line)
                    }
                    XCTAssertEqual(a.redComponent, e.redComponent, accuracy: 0.0001, file: file, line: line)
                    XCTAssertEqual(a.greenComponent, e.greenComponent, accuracy: 0.0001, file: file, line: line)
                    XCTAssertEqual(a.blueComponent, e.blueComponent, accuracy: 0.0001, file: file, line: line)
                    XCTAssertEqual(a.alphaComponent, e.alphaComponent, accuracy: 0.0001, file: file, line: line)
                }
            }
        }
        assertColor(MatraQuotaBand.stroke(for: 0.28, style: .hardStep), matches: MatraQuotaBand.emerald.color)
        assertColor(MatraQuotaBand.stroke(for: 0.81, style: .hardStep), matches: MatraQuotaBand.orange.color)
        assertColor(MatraQuotaBand.stroke(for: nil, style: .ramp), matches: MatraQuotaBand.unknown.color)
        for (fraction, expected) in [(0.0, MatraQuotaBand.emerald), (0.30, .amber), (0.60, .orange), (0.90, .red), (1.0, .red)] {
            assertColor(MatraQuotaBand.stroke(for: fraction, style: .ramp, watchLimit: 0.30, criticalLimit: 0.60),
                        matches: expected.color)
        }
        for style in ColorTransitionStyle.allCases {
            assertColor(MatraQuotaBand.stroke(for: 0.90, style: style, watchLimit: 0.95, criticalLimit: 0.99),
                        matches: MatraQuotaBand.red.color)
            assertColor(MatraQuotaBand.stroke(for: .nan, style: style), matches: MatraQuotaBand.unknown.color)
        }
        let overrides: [(UsageBand, MatraQuotaBand)] = [
            (.ample, .emerald), (.watch, .amber), (.critical, .orange), (.exhausted, .red)
        ]
        for (override, expected) in overrides {
            for style in ColorTransitionStyle.allCases {
                assertColor(MatraQuotaBand.stroke(for: 0.12, style: style, override: override), matches: expected.color)
            }
        }
    }

    func testApprovedThemesAndLegacyPersistence() {
        XCTAssertEqual(NotchSurfaceStyle.allCases.map(\.title), ["Liquid Glass", "Dark Glass", "Solid Dark", "Light", "System"])
        XCTAssertEqual(NotchSurfaceStyle(rawValue: "solid"), .solid)
        XCTAssertEqual(NotchSurfaceStyle(rawValue: "darkGlass"), .darkGlass)
        XCTAssertNil(NotchSurfaceStyle.system.panelAppearance(reduceTransparency: false))
        for reduce in [true, false] {
            XCTAssertEqual(NotchSurfaceStyle.light.panelAppearance(reduceTransparency: reduce)?.name, .aqua)
            XCTAssertEqual(NotchSurfaceStyle.glass.panelAppearance(reduceTransparency: reduce)?.name, .darkAqua)
            XCTAssertEqual(NotchSurfaceStyle.darkGlass.panelAppearance(reduceTransparency: reduce)?.name, .darkAqua)
            XCTAssertEqual(NotchSurfaceStyle.solid.panelAppearance(reduceTransparency: reduce)?.name, .darkAqua)
        }
    }

    func testGlassFallsBackToSolidDarkWithoutChangingStoredIdentity() {
        for style in [NotchSurfaceStyle.glass, .darkGlass] {
            XCTAssertEqual(style.effective(glassAvailable: false), .solid)
            XCTAssertEqual(style.effective(glassAvailable: true), style)
            XCTAssertEqual(NotchSurfaceStyle(rawValue: style.rawValue), style)
        }
        for style in [NotchSurfaceStyle.light, .system, .solid] {
            XCTAssertEqual(style.effective(glassAvailable: false), style)
        }
    }

    func testDarkBrandTextHasContrastOnSolidSurface() {
        func luminance(_ color: NSColor) -> CGFloat {
            func linear(_ c: CGFloat) -> CGFloat {
                c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
            }
            return 0.2126 * linear(color.redComponent)
                + 0.7152 * linear(color.greenComponent)
                + 0.0722 * linear(color.blueComponent)
        }
        NSAppearance(named: .darkAqua)!.performAsCurrentDrawingAppearance {
            guard let background = NSColor(Palette.card).usingColorSpace(.sRGB) else {
                return XCTFail("Background must resolve to sRGB")
            }
            for foreground in [Palette.textPrimary, Palette.textSecondary, MatraBrand.accent] {
                guard let color = NSColor(foreground).usingColorSpace(.sRGB) else {
                    return XCTFail("Foreground must resolve to sRGB")
                }
                XCTAssertGreaterThanOrEqual((luminance(color) + 0.05) / (luminance(background) + 0.05), 4.5)
            }
        }
    }

    @MainActor func testThemePersistsWithoutTouchingInstalledApp() {
        let name = "matra.test.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let preferences = Preferences(defaults: defaults)
        XCTAssertEqual(preferences.notchSurfaceStyle, .darkGlass)
        for theme in NotchSurfaceStyle.allCases {
            preferences.notchSurfaceStyle = theme
            XCTAssertEqual(Preferences(defaults: defaults).notchSurfaceStyle, theme)
        }
    }

    @MainActor func testOwnedUpdateChannelAndAutomaticInstallPreference() {
        XCTAssertTrue(Updater.isEnabled)
        XCTAssertEqual(Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") as? String,
                       "https://raw.githubusercontent.com/panditfloki/matra/main/updates/macos/appcast.xml")
        XCTAssertEqual(Bundle.main.object(forInfoDictionaryKey: "SUPublicEDKey") as? String,
                       "ZXK9jxpRx4EksQGRdhTgFSkbIOTCauZ0EhTe959OQ6I=")
        let name = "matra.updater.test.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let updater = Updater(defaults: defaults)
        XCTAssertFalse(updater.automaticallyInstallsUpdates)
        updater.automaticallyInstallsUpdates = true
        XCTAssertTrue(Updater(defaults: defaults).automaticallyInstallsUpdates)
        updater.automaticallyInstallsUpdates = false
        XCTAssertFalse(Updater(defaults: defaults).automaticallyInstallsUpdates)
    }

    func testBrandLinksAndCopy() {
        XCTAssertEqual(L10n.t("Working in \("matra")"), "Working in matra")
        XCTAssertEqual(L10n.branded("Quit Matra", locale: Locale(identifier: "id")), "Keluar dari Mātrā")
        XCTAssertEqual(MatraBrand.website.host, "dydxfx.com")
        XCTAssertEqual(MatraBrand.github.path, "/panditfloki")
        XCTAssertEqual(MatraBrand.repository.path, "/panditfloki/matra")
        XCTAssertEqual(MatraBrand.copy("Quit Matra"), "Quit Mātrā")
        XCTAssertEqual(MatraBrand.copy("MATRA Settings"), "Mātrā Settings")
        XCTAssertEqual(MatraBrand.copy("Mātrā"), "Mātrā")
        XCTAssertEqual(MatraBrand.copy("Matra releases at https://github.com/panditfloki/matra/releases?app=Matra"),
                       "Mātrā releases at https://github.com/panditfloki/matra/releases?app=Matra")
        XCTAssertEqual(MatraBrand.copy("Open /Applications/Matra.app and Matra/settings.json"),
                       "Open /Applications/Matra.app and Matra/settings.json")
        XCTAssertEqual(MatraBrand.copy("com.dydxfx.matra.mac"), "com.dydxfx.matra.mac")
    }

    @MainActor func testPreviewReadingsAreExplicitSamples() {
        let samples = MatraDesignPreview.samples()
        XCTAssertEqual(samples.count, 3)
        XCTAssertTrue(samples.allSatisfy { $0.displayName.contains("Sample") })
        XCTAssertNil(samples.last?.windows.first?.usedFraction)
    }
}

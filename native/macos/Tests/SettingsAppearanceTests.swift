import AppKit
import SwiftUI
import XCTest
@testable import Matra

@MainActor
final class SettingsAppearanceTests: XCTestCase {
    /// Hosts the real settings root, not just the enum's nil return value.
    /// SwiftUI's old preferredColorScheme could write Aqua back to this window.
    func testLightToSystemClearsTheHostingWindowOverride() throws {
        let suite = "SettingsAppearanceTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = Preferences(defaults: defaults)
        let previousAppearance = NSApp.appearance
        defer { NSApp.appearance = previousAppearance }
        // Only the isolated test host changes. The user's OS preference stays put.
        NSApp.appearance = NSAppearance(named: .darkAqua)
        preferences.notchSurfaceStyle = .light

        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: SettingsView.width,
                                                  height: SettingsView.height),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let subscription = SettingsWindowController.followAppearance(in: window, preferences: preferences)
        defer { subscription.cancel() }
        window.contentView = NSHostingView(rootView: SettingsView(
            preferences: preferences, providers: { [] },
            signOut: { _ in }, signIn: { _ in false }, switchAccount: { _ in false },
            retry: { _ in }, resetPosition: {}, quit: {}, updater: Updater(defaults: defaults)
        ))
        // Keep the integration host offscreen. Ordering an unrelated test window
        // frontmost can let SwiftUI's app lifecycle end the XCTest host when it
        // closes. Appearance inheritance does not require a visible window.

        func settle() {
            window.contentView?.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.10))
        }
        func assertAppearance(_ expected: NSAppearance.Name,
                              file: StaticString = #filePath, line: UInt = #line) {
            settle()
            XCTAssertEqual(window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]),
                           expected, file: file, line: line)
            XCTAssertEqual(window.contentView?.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]),
                           expected, file: file, line: line)
        }

        assertAppearance(.aqua)
        preferences.notchSurfaceStyle = .system
        assertAppearance(.darkAqua)
        XCTAssertNil(window.appearance, "System must inherit, not force either theme")

        // System must follow a later appearance change without reopening Settings.
        NSApp.appearance = NSAppearance(named: .aqua)
        assertAppearance(.aqua)
        NSApp.appearance = NSAppearance(named: .darkAqua)
        assertAppearance(.darkAqua)

        for style in [NotchSurfaceStyle.glass, .darkGlass, .solid] {
            preferences.notchSurfaceStyle = style
            assertAppearance(.darkAqua)
            preferences.notchSurfaceStyle = .light
            assertAppearance(.aqua)
            preferences.notchSurfaceStyle = .system
            assertAppearance(.darkAqua)
        }
    }
}

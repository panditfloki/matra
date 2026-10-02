import AppKit
import Combine
import SwiftUI

@MainActor
final class DashboardWindowController: NSObject, NSWindowDelegate {
    let model: DashboardModel
    let preferences: Preferences
    private(set) var window: NSWindow?
    private var appearanceSubscription: AnyCancellable?
    private let openSettings: () -> Void

    init(model: DashboardModel, preferences: Preferences, openSettings: @escaping () -> Void) {
        self.model = model
        self.preferences = preferences
        self.openSettings = openSettings
    }

    func show(providerID: String? = nil) {
        // A Dock reopen retains the current investigation; explicit provider
        // navigation changes it. An unknown/disconnected provider falls to Overview.
        if let providerID { model.select(providerID) }
        model.loadAnalytics()
        let window = prepareWindow()
        if !NSScreen.screens.contains(where: { $0.visibleFrame.intersects(window.frame) }) {
            window.center()
        }
        if window.isMiniaturized { window.deminiaturize(nil) }
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        window.orderFrontRegardless()
        SettingsWindowController.startUnfocused(window)
    }

    /// Window construction is separable from presentation for offscreen tests.
    @discardableResult
    func prepareWindow() -> NSWindow {
        if let window { return window }
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 680),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false)
        window.title = model.previewTitle.map { "Mātrā Dashboard · \($0)" } ?? "Mātrā Dashboard"
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = true
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 780, height: 520)
        window.delegate = self
        appearanceSubscription = SettingsWindowController.followAppearance(in: window, preferences: preferences)
        window.contentView = NSHostingView(rootView: DashboardView(
            model: model, preferences: preferences, openSettings: openSettings))
        window.setFrameAutosaveName(model.isPreview ? "MatraDashboardPreview" : "MatraDashboard")
        if !window.setFrameUsingName(window.frameAutosaveName) { window.center() }
        self.window = window
        AppWindowPresentation.register(window)
        layoutTrafficLights(window)
        return window
    }

    func windowWillClose(_ notification: Notification) {
        AppWindowPresentation.restore(preferences.appPresence, excluding: window)
    }

    func windowDidBecomeKey(_ notification: Notification) {
        if let window { layoutTrafficLights(window) }
    }

    func windowDidResize(_ notification: Notification) {
        if let window { layoutTrafficLights(window) }
    }

    private func layoutTrafficLights(_ window: NSWindow) {
        let buttons = [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
            .compactMap { window.standardWindowButton($0) }
        guard let container = buttons.first?.superview else { return }
        for (index, button) in buttons.enumerated() {
            button.setFrameOrigin(NSPoint(
                x: 26 + CGFloat(index) * 22.5 - button.frame.width / 2,
                y: container.bounds.height - SettingsView.headerHeight / 2 - button.frame.height / 2))
        }
    }
}

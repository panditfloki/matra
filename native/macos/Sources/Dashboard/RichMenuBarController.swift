import AppKit
import Combine
import SwiftUI

@MainActor
final class RichMenuBarController: NSObject, NSPopoverDelegate {
    let model: DashboardModel
    let preferences: Preferences
    /// Which card's detail is shown; shared by the popover and the panel.
    let hover = DetailHoverStore()
    private(set) var popover: NSPopover?
    /// The detail panel beside the popover, once a detail has been shown.
    private(set) var detailPanel: DetailSidePanel?
    private var appearance: AnyCancellable?
    private var detailAppearance: AnyCancellable?
    private var layoutChanges: AnyCancellable?
    private let openSettings: () -> Void
    private let openDashboard: (String?) -> Void
    /// The status button the popover hangs from, while it is shown.
    private weak var anchor: NSView?

    /// The popover is the reading column alone, whatever is hovered; the
    /// detail sits in its own panel beside it.
    static let compactWidth: CGFloat = MenuBarUsageView.readingWidth
    static let detailWidth: CGFloat = UsageDetailPanelView.width

    init(model: DashboardModel, preferences: Preferences,
         openSettings: @escaping () -> Void, openDashboard: @escaping (String?) -> Void) {
        self.model = model
        self.preferences = preferences
        self.openSettings = openSettings
        self.openDashboard = openDashboard
        super.init()
        hover.onShownChange = { [weak self] shown in
            if shown { self?.showDetailPanel() } else { self?.hideDetailPanel() }
        }
        // The side is chosen again whenever the popover or its screen changes.
        let center = NotificationCenter.default
        layoutChanges = Publishers.MergeMany([
            NSApplication.didChangeScreenParametersNotification, NSWindow.didMoveNotification,
            NSWindow.didResizeNotification, NSWindow.didChangeScreenNotification,
        ].map { center.publisher(for: $0) }).sink { [weak self] note in
            guard let self, self.detailPanel?.isVisible == true,
                  note.object is NSApplication || (note.object as? NSWindow) === self.hostingWindow else { return }
            self.positionDetailPanel()
        }
    }

    @discardableResult func preparePopover() -> NSPopover {
        if let popover { return popover }
        let popover = NSPopover()
        popover.behavior = .transient
        popover.delegate = self
        let hosting = NSHostingController(rootView: MenuBarUsageView(model: model, preferences: preferences, hover: hover,
            openSettings: { [weak self] in self?.close(); self?.openSettings() },
            openDashboard: { [weak self] id in self?.close(); self?.openDashboard(id) },
            close: { [weak self] in self?.close() }))
        popover.contentViewController = hosting
        // AppKit copies the view's size when assigning its controller.
        popover.contentSize = NSSize(width: Self.compactWidth, height: 650)
        appearance = preferences.$notchSurfaceStyle.sink { [weak popover, weak hosting] style in
            let appearance = style.panelAppearance(reduceTransparency: NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency)
            popover?.appearance = appearance
            hosting?.view.appearance = appearance
        }
        self.popover = popover
        return popover
    }

    func toggle(relativeTo button: NSStatusBarButton) {
        let popover = preparePopover()
        if popover.isShown { close(); return }
        let availableHeight = (button.window?.screen?.visibleFrame.height ?? 800) - 60
        popover.contentSize = NSSize(width: Self.compactWidth, height: min(650, max(420, availableHeight)))
        popover.animates = !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        model.loadAnalytics()
        anchor = button
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        hostingWindow?.title = model.previewTitle.map { "Mātrā Usage Panel · \($0)" } ?? "Mātrā Usage Panel"
    }

    private var hostingWindow: NSWindow? { popover?.contentViewController?.view.window }

    func close() {
        // A menu action must dismiss immediately before opening another app
        // window. Reopening restores the normal system/reduced-motion choice.
        popover?.animates = false
        popover?.close()
    }

    // MARK: Detail panel

    private func prepareDetailPanel() -> DetailSidePanel {
        if let detailPanel { return detailPanel }
        let panel = DetailSidePanel(rootView: UsageDetailPanelView(model: model, preferences: preferences, hover: hover))
        panel.onHover = { [weak self] inside in
            if inside { self?.hover.enterDetail() } else { self?.hover.leaveDetail() }
        }
        detailAppearance = SettingsWindowController.followAppearance(in: panel, preferences: preferences)
        detailPanel = panel
        return panel
    }

    private func showDetailPanel() {
        guard let popover, popover.isShown, let window = hostingWindow else { return }
        let panel = prepareDetailPanel()
        let wasVisible = panel.isVisible
        panel.level = window.level
        positionDetailPanel()
        if panel.parent !== window {
            panel.parent?.removeChildWindow(panel)
            // A child window moves, orders and closes with the popover.
            window.addChildWindow(panel, ordered: .above)
        }
        if !wasVisible && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            panel.alphaValue = 0
            panel.orderFront(nil)
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.12
                panel.animator().alphaValue = 1
            }
        } else {
            panel.alphaValue = 1
            panel.orderFront(nil)
        }
        // With room on neither side the panel overlaps the popover and can
        // open under a resting pointer, which sends no mouse-entered.
        if panel.frame.contains(NSEvent.mouseLocation) { hover.enterDetail() }
    }

    private func hideDetailPanel() {
        guard let panel = detailPanel else { return }
        panel.parent?.removeChildWindow(panel)
        panel.orderOut(nil)
        // A window ordered out from under the pointer sends no mouse-exited.
        if hover.state.pointerInDetail { hover.leaveDetail() }
    }

    /// Puts the panel flush beside the popover, on the side `DetailPanelPlacement`
    /// picks for the popover's current screen.
    private func positionDetailPanel() {
        guard let panel = detailPanel, let window = hostingWindow,
              let content = popover?.contentViewController?.view else { return }
        // The content view, not the window: the window also holds the arrow.
        let popoverFrame = window.convertToScreen(content.convert(content.bounds, to: nil))
        let screen = window.screen ?? anchor?.window?.screen ?? NSScreen.main
        let placement = DetailPanelPlacement.place(
            beside: popoverFrame,
            panelSize: NSSize(width: Self.detailWidth, height: popoverFrame.height),
            in: screen?.visibleFrame ?? popoverFrame)
        if panel.frame != placement.frame { panel.setFrame(placement.frame, display: true) }
    }

    // MARK: NSPopoverDelegate

    func popoverDidShow(_ notification: Notification) {
        if hover.state.isShown { showDetailPanel() }
    }

    /// A click in the detail panel is a click in the popover's own detail,
    /// not outside it, so it must not dismiss the transient popover.
    func popoverShouldClose(_ popover: NSPopover) -> Bool {
        guard let panel = detailPanel, panel.isVisible, let event = NSApp.currentEvent else { return true }
        switch event.type {
        case .leftMouseDown, .rightMouseDown, .otherMouseDown:
            return !panel.frame.contains(NSEvent.mouseLocation)
        default:
            return true
        }
    }

    func popoverWillClose(_ notification: Notification) {
        hover.reset()
        hideDetailPanel()
    }
}

import AppKit
import SwiftUI

/// One hover/pin state for the popover's cards and the detail panel beside
/// it: they live in two windows, so the state lives outside both views. Owns
/// the hide timer and the motion; the rules stay in `DetailHoverState`.
@MainActor
final class DetailHoverStore: ObservableObject {
    @Published private(set) var state = DetailHoverState()
    /// Told after the detail appears or goes, to show or hide the panel.
    var onShownChange: (Bool) -> Void = { _ in }

    func hoverCard(_ id: String) { update(animated: true) { $0.hoverCard(id) } }
    func leaveCard(_ id: String) { scheduleHide(update { $0.leaveCard(id) }) }
    func enterDetail() { update { $0.enterDetail() } }
    func leaveDetail() { scheduleHide(update { $0.leaveDetail() }) }
    func activateCard(_ id: String) { update(animated: true) { $0.activateCard(id) } }
    /// Returns false when there was nothing to dismiss.
    @discardableResult func escape() -> Bool { update(animated: true) { $0.escape() } }
    func prune(keeping ids: Set<String>) { update { $0.prune(keeping: ids) } }

    /// The popover closed: nothing is hovered, pinned or shown any more. A
    /// closing window sends no hover exits, so they are applied here.
    func reset() {
        update {
            $0.escape()
            if let id = $0.hoveredID { _ = $0.leaveCard(id) }
            if $0.pointerInDetail { _ = $0.leaveDetail() }
        }
    }

    private func scheduleHide(_ token: Int?) {
        guard let token else { return }
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: DetailHoverState.hideDelay)
            self?.update(animated: true) { $0.hideTimerFired(token) }
        }
    }

    /// Applies a change with the panel's motion, or none under Reduce Motion.
    @discardableResult
    private func update<T>(animated: Bool = false, _ change: (inout DetailHoverState) -> T) -> T {
        var next = state
        let result = change(&next)
        guard next != state else { return result }
        let wasShown = state.isShown
        if animated && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            withAnimation(.easeOut(duration: 0.18)) { state = next }
        } else {
            state = next
        }
        if next.isShown != wasShown { onShownChange(next.isShown) }
        return result
    }
}

/// The hovered or pinned provider's detail, shown in the panel beside the popover.
struct UsageDetailPanelView: View {
    /// The detail column's width from when it sat inside the 720-point popover:
    /// that popover less the reading column and a hairline. Its header, tokens
    /// and cost row and chart are laid out for this width.
    static let width: CGFloat = 349

    @ObservedObject var model: DashboardModel
    @ObservedObject var preferences: Preferences
    @ObservedObject var hover: DetailHoverStore
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.matraReduceTransparency) private var reduceTransparency

    private var inspected: ProviderSnapshot? {
        hover.state.shownID.flatMap { id in model.snapshots.first { $0.id == id } }
    }

    var body: some View {
        ScrollView {
            if let snapshot = inspected { detail(snapshot) }
        }
        .foregroundStyle(Palette.textPrimary)
        .modifier(MatraWindowSurface(preferences: preferences))
        .environment(\.notchSurfaceStyle, preferences.notchSurfaceStyle)
        .environment(\.tooltipSecondaryInk, TooltipGlassContrast.secondaryInk(
            surfaceStyle: preferences.notchSurfaceStyle, colorScheme: colorScheme, reduceTransparency: reduceTransparency))
    }

    private func detail(_ snapshot: ProviderSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Label(snapshot.displayName, systemImage: "chart.bar.xaxis")
                    .font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Spacer()
                if hover.state.pinnedID == snapshot.id {
                    Button { hover.escape() } label: { Image(systemName: "pin.slash") }
                        .buttonStyle(.plain).help("Unpin details (Esc)")
                        .accessibilityLabel("Unpin details")
                }
            }
            if let report = model.report(for: snapshot) {
                UsageSpendView(report: report, compact: true).id(snapshot.id)
            } else {
                UsageAnalyticsState(model: model, snapshot: snapshot)
            }
        }.padding(18).frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// What a card or the detail says while a source has no token/cost report.
struct UsageAnalyticsState: View {
    @ObservedObject var model: DashboardModel
    let snapshot: ProviderSnapshot

    var body: some View {
        let id = DashboardModel.analyticsID(snapshot)
        if model.analyticsLoading.contains(id) {
            HStack { ProgressView().controlSize(.small); Text("Reading local usage…").font(.system(size: 11)) }
        } else {
            Text(model.analyticsErrors[id] ?? "Token/cost analytics are not available for this source yet.")
                .font(.system(size: 11)).foregroundStyle(Palette.textSecondary).fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// The detail panel's window. It floats beside the popover as its child, so
/// it moves and closes with it, and it never becomes key: Escape, typing and
/// VoiceOver focus stay with the popover.
final class DetailSidePanel: NSPanel {
    /// Pointer entered (true) or left (false) the panel.
    var onHover: (Bool) -> Void {
        get { tracker.onHover }
        set { tracker.onHover = newValue }
    }
    private let tracker = HoverTrackingView()

    init<Content: View>(rootView: Content) {
        super.init(contentRect: NSRect(x: 0, y: 0, width: UsageDetailPanelView.width, height: 420),
                   styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        title = "Mātrā Usage Details"
        collectionBehavior = [.fullScreenAuxiliary, .ignoresCycle]
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        isMovable = false
        isMovableByWindowBackground = false
        // The popover decides when both go; an accessory app is often not
        // the active one while its popover is open.
        hidesOnDeactivate = false
        becomesKeyOnlyIfNeeded = true
        isReleasedWhenClosed = false
        animationBehavior = .none
        let hosting = FirstMouseHostingView(rootView: rootView)
        // The controller sizes the panel; the content scrolls inside it.
        hosting.sizingOptions = []
        tracker.frame = NSRect(origin: .zero, size: frame.size)
        hosting.frame = tracker.bounds
        hosting.autoresizingMask = [.width, .height]
        tracker.addSubview(hosting)
        contentView = tracker
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

/// Reports the pointer entering and leaving, whatever window is key.
private final class HoverTrackingView: NSView {
    var onHover: (Bool) -> Void = { _ in }
    private var area: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let area { removeTrackingArea(area) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        self.area = area
    }

    override func mouseEntered(with event: NSEvent) { onHover(true) }
    override func mouseExited(with event: NSEvent) { onHover(false) }
}

/// The panel is never key, so its chart and pickers must take the first click.
private final class FirstMouseHostingView<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

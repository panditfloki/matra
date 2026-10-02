import AppKit
import SwiftUI
import XCTest
@testable import Matra

/// The popover's provider tabs: every tab answers a click anywhere inside it,
/// and at the popover's width all five of the owner's tabs are on screen.
@MainActor
final class UsageTabStripTests: XCTestCase {
    private let ownerTabs: [UsageTabStrip.Item] = [
        .init(id: nil, name: "Overview", glyph: nil),
        .init(id: "claude", name: "Claude Tutamail", glyph: .claude),
        .init(id: "codex", name: "Codex", glyph: .openai),
        .init(id: "cursor", name: "Cursor", glyph: .cursor),
        .init(id: "antigravity", name: "Antigravity", glyph: .antigravity),
    ]

    private func host<V: View>(_ view: V, size: NSSize) -> (NSWindow, NSHostingView<V>) {
        let hosting = NSHostingView(rootView: view)
        let window = NSWindow(contentRect: NSRect(origin: NSPoint(x: 80, y: 80), size: size),
                              styleMask: .borderless, backing: .buffered, defer: false)
        window.contentView = hosting
        window.orderFrontRegardless()
        hosting.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        return (window, hosting)
    }

    /// A real click, through the window, at `point` in window coordinates.
    private func click(_ window: NSWindow, at point: NSPoint) {
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil,
                                           eventNumber: 0, clickCount: 1, pressure: 1)!
            window.sendEvent(event)
            RunLoop.main.run(until: Date().addingTimeInterval(0.03))
        }
    }

    /// The old tab drew only its mark and name when unselected, and a plain
    /// button is hit only where it draws, so a click on the tab's own padding
    /// did nothing. That was the "hard to click" Codex, Cursor and Antigravity.
    func testAnUnselectedTabAnswersAClickOnItsPadding() throws {
        var tapped = 0
        let tab = UsageTab(item: ownerTabs[2], selected: false, accent: .blue) { tapped += 1 }
        let probe = NSHostingView(rootView: tab)
        let size = probe.fittingSize
        let (window, _) = host(tab, size: size)
        defer { window.orderOut(nil) }
        // 3 pt in from each side edge, at mid-height: clear of the mark and the name.
        click(window, at: NSPoint(x: 3, y: size.height / 2))
        click(window, at: NSPoint(x: size.width - 3, y: size.height / 2))
        XCTAssertEqual(tapped, 2)
    }

    /// At 370 points the regular row of the owner's five tabs ran to 460, which
    /// put Antigravity wholly past the popover's edge. Scanning from the
    /// popover's right edge, the first tab that answers must be the last one.
    func testTheLastTabIsOnScreenAndClickableAtThePopoverWidth() throws {
        var selected: [String?] = []
        let width = MenuBarUsageView.readingWidth
        let strip = UsageTabStrip(items: ownerTabs, selectedID: nil, accent: .blue) { selected.append($0) }
            .frame(width: width)
        let height = NSHostingView(rootView: strip).fittingSize.height
        let (window, _) = host(strip, size: NSSize(width: width, height: height))
        defer { window.orderOut(nil) }
        var x = width - 1
        while selected.isEmpty && x > 0 {
            click(window, at: NSPoint(x: x, y: height / 2))
            x -= 2
        }
        XCTAssertEqual(selected.first ?? nil, "antigravity", "first tab hit from the right edge")
    }

    func testTheCompactRowIsTheOneThatFitsTheOwnersTabs() {
        func width(_ metrics: UsageTabRow.Metrics) -> CGFloat {
            NSHostingView(rootView: UsageTabRow(items: ownerTabs, selectedID: nil, accent: .blue,
                                                metrics: metrics, select: { _ in })).fittingSize.width
        }
        XCTAssertGreaterThan(width(.regular), MenuBarUsageView.readingWidth, "why the row tightens")
        XCTAssertLessThanOrEqual(width(.compact), MenuBarUsageView.readingWidth)
    }
}

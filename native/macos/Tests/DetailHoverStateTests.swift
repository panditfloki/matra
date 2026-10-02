import AppKit
import SwiftUI
import XCTest
@testable import Matra

/// The usage popover's detail: hidden until a card is hovered or pinned, kept
/// open while the pointer crosses into the detail panel, gone ~300 ms after.
final class DetailHoverStateTests: XCTestCase {
    func testHiddenByDefault() {
        let state = DetailHoverState()
        XCTAssertNil(state.shownID)
        XCTAssertFalse(state.isShown)
    }

    func testHoverShowsAndLeavingHidesOnlyAfterTheTimer() throws {
        var state = DetailHoverState()
        state.hoverCard("claude")
        XCTAssertEqual(state.shownID, "claude")
        let token = try XCTUnwrap(state.leaveCard("claude"))
        XCTAssertEqual(state.shownID, "claude", "not hidden until the delay passes")
        state.hideTimerFired(token)
        XCTAssertNil(state.shownID)
        XCTAssertEqual(DetailHoverState.hideDelay, .milliseconds(300))
    }

    func testMovingFromCardIntoDetailKeepsItOpen() throws {
        var state = DetailHoverState()
        state.hoverCard("claude")
        let token = try XCTUnwrap(state.leaveCard("claude"))
        state.enterDetail()
        state.hideTimerFired(token)
        XCTAssertEqual(state.shownID, "claude", "the chart's controls stay usable")
        let second = try XCTUnwrap(state.leaveDetail())
        state.hideTimerFired(second)
        XCTAssertNil(state.shownID)
    }

    func testReturningBeforeTheTimerCancelsTheHide() throws {
        var state = DetailHoverState()
        state.hoverCard("claude")
        let token = try XCTUnwrap(state.leaveCard("claude"))
        state.hoverCard("claude")
        state.hideTimerFired(token)
        XCTAssertEqual(state.shownID, "claude")
    }

    func testHoveringAnotherCardSwapsTheDetailAndStaleTimersDoNothing() throws {
        var state = DetailHoverState()
        state.hoverCard("claude")
        let stale = try XCTUnwrap(state.leaveCard("claude"))
        state.hoverCard("codex")
        XCTAssertEqual(state.shownID, "codex")
        state.hideTimerFired(stale)
        XCTAssertEqual(state.shownID, "codex")
        XCTAssertNil(state.leaveCard("claude"), "a late exit from the old card is ignored")
    }

    func testClickPinsUntilClickedAgain() throws {
        var state = DetailHoverState()
        state.hoverCard("claude")
        state.activateCard("claude")
        XCTAssertEqual(state.pinnedID, "claude")
        XCTAssertNil(state.leaveCard("claude"), "pinned: leaving arms no hide")
        state.hoverCard("codex")
        XCTAssertEqual(state.shownID, "claude", "hover does not replace a pinned detail")
        state.activateCard("codex")
        XCTAssertEqual(state.shownID, "codex", "clicking another card moves the pin")
        state.activateCard("codex")
        XCTAssertNil(state.pinnedID)
        XCTAssertEqual(state.shownID, "codex", "unpinned under the pointer, still shown")
        let token = try XCTUnwrap(state.leaveCard("codex"))
        state.hideTimerFired(token)
        XCTAssertNil(state.shownID)
    }

    func testKeyboardActivationWithoutPointerPinsAndUnpins() {
        var state = DetailHoverState()
        state.activateCard("claude")
        XCTAssertEqual(state.shownID, "claude")
        state.activateCard("claude")
        XCTAssertNil(state.shownID, "no pointer to hold it open")
    }

    func testEscapeUnpinsAndHidesAndReportsWhetherItDidAnything() {
        var state = DetailHoverState()
        XCTAssertFalse(state.escape(), "nothing to dismiss: caller closes the popover")
        state.activateCard("claude")
        XCTAssertTrue(state.escape())
        XCTAssertNil(state.shownID)
        XCTAssertNil(state.pinnedID)
    }

    func testPruneDropsProvidersThatDisappear() {
        var state = DetailHoverState()
        state.activateCard("claude")
        state.prune(keeping: ["codex"])
        XCTAssertNil(state.pinnedID)
        XCTAssertNil(state.shownID)
    }
}

/// The store the popover's cards and the detail panel share: it runs the
/// hide timer and tells the controller when to show or hide the panel.
@MainActor final class DetailHoverStoreTests: XCTestCase {
    func testCrossingFromCardToPanelNeverHidesAndLeavingBothDoes() async throws {
        let store = DetailHoverStore()
        var changes: [Bool] = []
        store.onShownChange = { changes.append($0) }
        store.hoverCard("claude")
        XCTAssertEqual(changes, [true])
        store.leaveCard("claude")
        store.enterDetail()
        try await Task.sleep(for: DetailHoverState.hideDelay + .milliseconds(200))
        XCTAssertEqual(store.state.shownID, "claude", "the armed hide was cancelled by the panel")
        XCTAssertEqual(changes, [true])
        store.leaveDetail()
        for _ in 0..<40 where store.state.isShown { try await Task.sleep(for: .milliseconds(50)) }
        XCTAssertNil(store.state.shownID)
        XCTAssertEqual(changes, [true, false])
    }

    func testSwappingCardsTellsTheControllerOnlyOnce() {
        let store = DetailHoverStore()
        var changes: [Bool] = []
        store.onShownChange = { changes.append($0) }
        store.hoverCard("claude")
        store.leaveCard("claude")
        store.hoverCard("codex")
        XCTAssertEqual(store.state.shownID, "codex")
        XCTAssertEqual(changes, [true], "the panel stays up; only its content changes")
    }

    func testEscapeReportsWhetherItDismissedAnything() {
        let store = DetailHoverStore()
        XCTAssertFalse(store.escape(), "nothing shown: the popover closes instead")
        store.activateCard("claude")
        XCTAssertTrue(store.escape())
        XCTAssertNil(store.state.pinnedID)
    }

    /// A closing popover sends no hover exits, so a reset must leave nothing
    /// behind that would keep the next detail from hiding.
    func testResetClearsPinHoverAndPointerSoTheNextHoverHidesNormally() async throws {
        let store = DetailHoverStore()
        var changes: [Bool] = []
        store.onShownChange = { changes.append($0) }
        store.hoverCard("claude")
        store.activateCard("claude")
        store.enterDetail()
        store.reset()
        XCTAssertNil(store.state.shownID)
        XCTAssertNil(store.state.pinnedID)
        XCTAssertNil(store.state.hoveredID)
        XCTAssertFalse(store.state.pointerInDetail)
        XCTAssertNil(store.state.pendingHide)
        XCTAssertEqual(changes, [true, false])
        store.hoverCard("codex")
        store.leaveCard("codex")
        for _ in 0..<40 where store.state.isShown { try await Task.sleep(for: .milliseconds(50)) }
        XCTAssertNil(store.state.shownID, "no stale pointer-in-panel holds it open")
    }
}

/// Where the detail panel sits beside the popover. AppKit screen coordinates:
/// origin bottom-left, so a menu-bar popover hangs from the top of `visible`.
final class DetailPanelPlacementTests: XCTestCase {
    private let visible = CGRect(x: 0, y: 0, width: 1512, height: 944)
    private let panel = CGSize(width: 349, height: 650)

    private func popoverFrame(minX: CGFloat) -> CGRect {
        CGRect(x: minX, y: visible.maxY - 650, width: 370, height: 650)
    }

    func testRightWhenOnlyTheRightFits() {
        let popover = popoverFrame(minX: 100)
        let placed = DetailPanelPlacement.place(beside: popover, panelSize: panel, in: visible)
        XCTAssertEqual(placed.side, .right)
        XCTAssertEqual(placed.frame, CGRect(x: popover.maxX, y: popover.minY, width: 349, height: 650))
    }

    func testLeftWhenOnlyTheLeftFits() {
        // A status item near the right end of the menu bar, as in the owner's reference.
        let popover = popoverFrame(minX: visible.maxX - 370 - 8)
        let placed = DetailPanelPlacement.place(beside: popover, panelSize: panel, in: visible)
        XCTAssertEqual(placed.side, .left)
        XCTAssertEqual(placed.frame.maxX, popover.minX, "flush against the popover's left edge")
        XCTAssertEqual(placed.frame.maxY, popover.maxY, "top edges level")
        XCTAssertEqual(placed.frame.size, panel)
    }

    func testRightIsPreferredWhenBothSidesFit() {
        let popover = popoverFrame(minX: 500)
        let placed = DetailPanelPlacement.place(beside: popover, panelSize: panel, in: visible)
        XCTAssertEqual(placed.side, .right)
        XCTAssertEqual(placed.frame.minX, popover.maxX)
    }

    func testAnExactFitOnTheRightStillCountsAsFitting() {
        let popover = popoverFrame(minX: visible.maxX - 370 - 349)
        let placed = DetailPanelPlacement.place(beside: popover, panelSize: panel, in: visible)
        XCTAssertEqual(placed.side, .right)
        XCTAssertEqual(placed.frame.maxX, visible.maxX)
        let onePointLess = DetailPanelPlacement.place(beside: popover.offsetBy(dx: 1, dy: 0), panelSize: panel, in: visible)
        XCTAssertEqual(onePointLess.side, .left)
    }

    func testNeitherFitsTakesTheRoomierSideAndStaysOnScreen() {
        let narrow = CGRect(x: 0, y: 0, width: 600, height: 700)
        // 150 points free on the left, 80 on the right.
        let nearRight = CGRect(x: 150, y: 50, width: 370, height: 650)
        let left = DetailPanelPlacement.place(beside: nearRight, panelSize: panel, in: narrow)
        XCTAssertEqual(left.side, .left)
        XCTAssertEqual(left.frame.minX, narrow.minX, "clamped: it overlaps the popover instead of leaving the screen")
        XCTAssertTrue(narrow.contains(left.frame))
        // 60 points free on the left, 170 on the right.
        let nearLeft = CGRect(x: 60, y: 50, width: 370, height: 650)
        let right = DetailPanelPlacement.place(beside: nearLeft, panelSize: panel, in: narrow)
        XCTAssertEqual(right.side, .right)
        XCTAssertEqual(right.frame.maxX, narrow.maxX)
        XCTAssertTrue(narrow.contains(right.frame))
    }

    func testAPanelLargerThanTheScreenIsCutDownToIt() {
        let tiny = CGRect(x: 0, y: 0, width: 300, height: 400)
        let popover = CGRect(x: 0, y: 0, width: 300, height: 400)
        let placed = DetailPanelPlacement.place(beside: popover, panelSize: panel, in: tiny)
        XCTAssertEqual(placed.frame, tiny)
    }

    func testHeightAndTopAreClampedIntoTheVisibleFrame() {
        // The popover reaches below the Dock line; the panel must not.
        let visible = CGRect(x: 0, y: 80, width: 1512, height: 600)
        let popover = CGRect(x: 100, y: 40, width: 370, height: 650)
        let placed = DetailPanelPlacement.place(beside: popover, panelSize: panel, in: visible)
        XCTAssertEqual(placed.frame.height, 600)
        XCTAssertEqual(placed.frame.minY, visible.minY)
        XCTAssertEqual(placed.frame.maxY, visible.maxY)
    }

    func testASecondDisplayUsesItsOwnVisibleFrame() {
        // A display arranged to the left of the main one has negative x.
        let second = CGRect(x: -1920, y: 120, width: 1920, height: 1055)
        let nearItsRightEdge = CGRect(x: -400, y: second.maxY - 650, width: 370, height: 650)
        let placed = DetailPanelPlacement.place(beside: nearItsRightEdge, panelSize: panel, in: second)
        XCTAssertEqual(placed.side, .left, "the main display's room on the right does not count")
        XCTAssertEqual(placed.frame.maxX, nearItsRightEdge.minX)
        XCTAssertTrue(second.contains(placed.frame))
        let nearItsLeftEdge = CGRect(x: -1900, y: second.maxY - 650, width: 370, height: 650)
        XCTAssertEqual(DetailPanelPlacement.place(beside: nearItsLeftEdge, panelSize: panel, in: second).side, .right)
    }

    func testAGapIsKeptOnEitherSide() {
        let right = DetailPanelPlacement.place(beside: popoverFrame(minX: 100), panelSize: panel, in: visible, gap: 6)
        XCTAssertEqual(right.frame.minX, popoverFrame(minX: 100).maxX + 6)
        let rightEdge = popoverFrame(minX: visible.maxX - 370)
        let left = DetailPanelPlacement.place(beside: rightEdge, panelSize: panel, in: visible, gap: 6)
        XCTAssertEqual(left.frame.maxX, rightEdge.minX - 6)
    }
}

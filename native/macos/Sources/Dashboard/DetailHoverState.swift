import Foundation

/// Which provider's detail the usage popover shows beside its cards.
///
/// Hidden by default. Hovering a card shows its detail; the pointer may cross
/// into the detail and keep it open; leaving both arms a short hide, so a
/// pointer passing over the gap does not flicker it shut. A click (or the
/// accessibility action) pins a card's detail until it is clicked again or
/// Escape is pressed. Kept free of views and timers so the rules can be tested.
struct DetailHoverState: Equatable {
    /// How long the detail stays after the pointer leaves both card and detail.
    static let hideDelay: Duration = .milliseconds(300)

    private(set) var shownID: String?
    private(set) var pinnedID: String?
    private(set) var hoveredID: String?
    private(set) var pointerInDetail = false
    /// The armed hide, if any. A newer arm or any return of the pointer
    /// invalidates older ones, so only the latest timer can hide.
    private(set) var pendingHide: Int?
    private var hideGeneration = 0

    var isShown: Bool { shownID != nil }

    mutating func hoverCard(_ id: String) {
        hoveredID = id
        pendingHide = nil
        // A pinned detail stays put; that is what pinning is for.
        if pinnedID == nil { shownID = id }
    }

    /// Returns the token to pass to `hideTimerFired` after `hideDelay`, or nil
    /// when nothing needs hiding.
    mutating func leaveCard(_ id: String) -> Int? {
        guard hoveredID == id else { return nil }
        hoveredID = nil
        return armHideIfIdle()
    }

    mutating func enterDetail() {
        pointerInDetail = true
        pendingHide = nil
    }

    mutating func leaveDetail() -> Int? {
        pointerInDetail = false
        return armHideIfIdle()
    }

    mutating func hideTimerFired(_ token: Int) {
        guard pendingHide == token, pinnedID == nil, hoveredID == nil, !pointerInDetail else { return }
        pendingHide = nil
        shownID = nil
    }

    /// Click or accessibility action on a card: pin it, or unpin it if it is
    /// already the pinned one.
    mutating func activateCard(_ id: String) {
        pendingHide = nil
        if pinnedID == id {
            pinnedID = nil
            // Still under the pointer after a click, so it stays shown until
            // the pointer leaves; without a pointer (keyboard), it hides now.
            shownID = hoveredID == id || pointerInDetail ? id : nil
        } else {
            pinnedID = id
            shownID = id
        }
    }

    /// Escape: unpin and hide. Returns false when there was nothing to
    /// dismiss, so the caller can fall back to closing the popover.
    @discardableResult mutating func escape() -> Bool {
        guard shownID != nil || pinnedID != nil else { return false }
        pinnedID = nil
        shownID = nil
        pendingHide = nil
        return true
    }

    /// Drops any reference to a provider that is no longer listed.
    mutating func prune(keeping ids: Set<String>) {
        if let pinnedID, !ids.contains(pinnedID) { self.pinnedID = nil }
        if let hoveredID, !ids.contains(hoveredID) { self.hoveredID = nil }
        if let shownID, !ids.contains(shownID) { self.shownID = pinnedID ?? hoveredID }
    }

    private mutating func armHideIfIdle() -> Int? {
        guard pinnedID == nil, hoveredID == nil, !pointerInDetail, shownID != nil else { return nil }
        hideGeneration += 1
        pendingHide = hideGeneration
        return hideGeneration
    }
}

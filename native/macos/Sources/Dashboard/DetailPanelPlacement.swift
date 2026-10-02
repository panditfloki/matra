import CoreGraphics

/// Where the usage popover's detail panel goes: beside the popover, flush on
/// the right when it fits on that screen, otherwise flush on the left, and
/// always inside the screen's visible frame. Pure geometry in AppKit screen
/// coordinates (origin bottom-left), so the rules can be tested without windows.
enum DetailPanelPlacement {
    enum Side: Equatable { case left, right }

    struct Result: Equatable {
        let side: Side
        let frame: CGRect
    }

    static func place(beside popover: CGRect, panelSize: CGSize, in visible: CGRect, gap: CGFloat = 0) -> Result {
        let width = min(panelSize.width, visible.width)
        let height = min(panelSize.height, visible.height)
        let rightX = popover.maxX + gap
        let leftX = popover.minX - gap - width
        let side: Side
        if rightX + width <= visible.maxX {
            side = .right
        } else if leftX >= visible.minX {
            side = .left
        } else {
            // Neither side has room: take the roomier one. The clamp below
            // then lets the panel overlap the popover rather than leave the screen.
            side = visible.maxX - popover.maxX >= popover.minX - visible.minX ? .right : .left
        }
        let x = clamp(side == .right ? rightX : leftX, visible.minX, visible.maxX - width)
        // Top edges level; moved only as far as it takes to stay on screen.
        let y = clamp(popover.maxY - height, visible.minY, visible.maxY - height)
        return Result(side: side, frame: CGRect(x: x, y: y, width: width, height: height))
    }

    private static func clamp(_ value: CGFloat, _ lower: CGFloat, _ upper: CGFloat) -> CGFloat {
        min(max(value, lower), max(lower, upper))
    }
}

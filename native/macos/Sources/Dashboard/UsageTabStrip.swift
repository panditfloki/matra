import SwiftUI

/// The provider tabs across the top of the usage popover.
///
/// Every tab answers a click anywhere inside it, as the notch's provider cells
/// do, not only on its mark and name. And the row tightens before it scrolls:
/// at the popover's 370 points the regular row of five tabs runs past the
/// right edge, which leaves the last tab reachable only by scrolling sideways.
struct UsageTabStrip: View {
    struct Item: Identifiable, Equatable {
        /// Nil for the Overview tab.
        let id: String?
        let name: String
        let glyph: ProviderGlyph?
    }

    let items: [Item]
    let selectedID: String?
    let accent: Color
    let select: (String?) -> Void

    var body: some View {
        ViewThatFits(in: .horizontal) {
            row(.regular)
            row(.compact)
            ScrollView(.horizontal) { row(.compact) }.scrollIndicators(.hidden)
        }
    }

    private func row(_ metrics: UsageTabRow.Metrics) -> UsageTabRow {
        UsageTabRow(items: items, selectedID: selectedID, accent: accent, metrics: metrics, select: select)
    }
}

/// One row of tabs at one density.
struct UsageTabRow: View {
    struct Metrics: Equatable {
        var minWidth: CGFloat
        var inset: CGFloat
        var spacing: CGFloat
        var edge: CGFloat

        static let regular = Metrics(minWidth: 68, inset: 8, spacing: 4, edge: 12)
        /// Same marks, type and height, with less air around each tab.
        static let compact = Metrics(minWidth: 48, inset: 6, spacing: 2, edge: 8)
    }

    let items: [UsageTabStrip.Item]
    let selectedID: String?
    let accent: Color
    let metrics: Metrics
    let select: (String?) -> Void

    var body: some View {
        HStack(spacing: metrics.spacing) {
            ForEach(items) { item in
                UsageTab(item: item, selected: selectedID == item.id, accent: accent,
                         minWidth: metrics.minWidth, inset: metrics.inset) { select(item.id) }
            }
        }
        .padding(.horizontal, metrics.edge)
        .padding(.vertical, 12)
    }
}

struct UsageTab: View {
    let item: UsageTabStrip.Item
    let selected: Bool
    let accent: Color
    var minWidth: CGFloat = UsageTabRow.Metrics.regular.minWidth
    var inset: CGFloat = UsageTabRow.Metrics.regular.inset
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 5) {
                if let glyph = item.glyph { ProviderGlyphView(glyph: glyph, size: 16) }
                else { Image(systemName: "square.grid.2x2").frame(height: 16) }
                Text(item.name).font(.system(size: 10, weight: .medium)).lineLimit(1)
            }
            .frame(minWidth: minWidth).padding(.horizontal, inset).padding(.vertical, 7)
            .background(selected ? accent.opacity(0.15) : .clear, in: RoundedRectangle(cornerRadius: 8))
            // A plain button is hit only where it draws; an unselected tab
            // draws only its mark and name, so most of it ignored clicks.
            .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

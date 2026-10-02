import SwiftUI

/// One compact reading column, always the same width. Hovering or pinning a
/// card (see `DetailHoverState`) highlights it and shows its detail in a
/// separate panel beside the popover (`DetailSidePanel`), which the
/// controller places; the popover itself never grows.
struct MenuBarUsageView: View {
    static let readingWidth: CGFloat = 370

    @ObservedObject var model: DashboardModel
    @ObservedObject var preferences: Preferences
    /// Shared with the detail panel. Not observed here: a hover change must
    /// not rebuild the tab strip under the pointer.
    var hover: DetailHoverStore
    let openSettings: () -> Void
    let openDashboard: (String?) -> Void
    let close: () -> Void
    @State private var selectedID: String?
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.matraReduceTransparency) private var reduceTransparency

    private var visible: [ProviderSnapshot] {
        if let selectedID { return model.snapshots.filter { $0.id == selectedID } }
        return model.snapshots
    }

    var body: some View {
        VStack(spacing: 0) {
            tabStrip
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    if model.isLivePreview {
                        Label("Live preview · Real data. Keep the installed Mātrā quit.", systemImage: "dot.radiowaves.left.and.right")
                            .font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if model.readsLocalLogs {
                        Label("Local-log preview · Your real local logs. Quotas are not polled.", systemImage: "doc.text.magnifyingglass")
                            .font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if model.isPreview {
                        Label("Sample preview", systemImage: "testtube.2")
                            .font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
                    }
                    if selectedID == nil { combinedSummary }
                    if model.snapshots.isEmpty {
                        Text("Waiting for the first reading…").font(.system(size: 12))
                    }
                    ForEach(visible) { snapshot in
                        MenuBarProviderCard(hover: hover, snapshotID: snapshot.id,
                                            accent: preferences.accentColor.color,
                                            glass: preferences.notchSurfaceStyle.isGlass) {
                            reading(snapshot)
                        }
                    }
                }.padding(16).frame(maxWidth: .infinity, alignment: .leading)
            }
            // Fixed in every state: a hosted popover otherwise follows its content's width.
            .frame(width: Self.readingWidth)
            Divider()
            footer
        }
        .foregroundStyle(Palette.textPrimary)
        .modifier(MatraWindowSurface(preferences: preferences))
        .environment(\.notchSurfaceStyle, preferences.notchSurfaceStyle)
        .environment(\.expandsUsageRows, true)
        .environment(\.tooltipSecondaryInk, TooltipGlassContrast.secondaryInk(
            surfaceStyle: preferences.notchSurfaceStyle, colorScheme: colorScheme, reduceTransparency: reduceTransparency))
        .environment(\.usageWatchLimit, preferences.watchLimit)
        .environment(\.usageCriticalLimit, preferences.criticalLimit)
        .environment(\.colorTransitionStyle, preferences.colorTransitionStyle)
        .onAppear { model.loadAnalytics() }
        .onChange(of: model.snapshots.map(\.id)) { _, ids in
            if let selectedID, !ids.contains(selectedID) { self.selectedID = nil }
            hover.prune(keeping: Set(ids))
        }
        .onExitCommand {
            // Escape first unpins and hides the detail; with none, it closes.
            if !hover.escape() { close() }
        }
    }

    private var tabStrip: some View {
        let items = [UsageTabStrip.Item(id: nil, name: "Overview", glyph: nil)] + model.snapshots.map {
            UsageTabStrip.Item(id: $0.id,
                               name: $0.displayName.replacingOccurrences(of: " · Sample", with: "")
                                   .replacingOccurrences(of: " · Local logs", with: ""),
                               glyph: $0.glyph)
        }
        return UsageTabStrip(items: items, selectedID: selectedID, accent: preferences.accentColor.color) {
            selectedID = $0
        }
    }

    private var combinedSummary: some View {
        let enabled = Set(model.snapshots.map(DashboardModel.analyticsID))
        let reports = model.spendReports.values.filter { enabled.contains($0.providerID) }
        let value = UsageSpendReport.Value.sum(reports.map(\.total))
        return VStack(alignment: .leading, spacing: 7) {
            Text("Usage & Spend · 30 days").font(.system(size: 12, weight: .semibold)).foregroundStyle(Palette.textSecondary)
            Text(SpendCopy.cost(reports.isEmpty ? nil : value)).font(.system(size: 24, weight: .semibold))
            Text("\(SpendCopy.tokens(reports.isEmpty ? nil : value)) tokens · \(reports.count) of \(enabled.count) local sources")
                .font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
            Text("Recorded local activity, not account-wide usage. Costs are USD API-price estimates.")
                .font(.system(size: 10)).foregroundStyle(Palette.textSecondary).fixedSize(horizontal: false, vertical: true)
            if let stale = SpendCopy.staleNote(reports) {
                Label(stale, systemImage: "clock").font(.system(size: 10)).foregroundStyle(Palette.watch)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func reading(_ snapshot: ProviderSnapshot) -> some View {
        TimelineView(.periodic(from: .now, by: 30)) { context in
            VStack(alignment: .leading, spacing: 16) {
                ProviderTooltip(snapshot: snapshot, now: context.date,
                    resetTimeFormat: preferences.resetTimeFormat, showUsagePace: preferences.showUsagePace)
                if let credits = snapshot.availableResetCredits(at: context.date) {
                    UsageResetCreditsSection(credits: credits, now: context.date)
                }
                if let report = model.report(for: snapshot) {
                    Divider()
                    UsageSpendSummary(report: report)
                    RecentUsageWindows(report: report, snapshot: snapshot)
                } else {
                    UsageAnalyticsState(model: model, snapshot: snapshot)
                }
                if let tokens = snapshot.tokenUsage {
                    Divider()
                    Text(L10n.t("Account-wide token activity")).font(.system(size: 11, weight: .semibold))
                    CodexUsageSection(usage: tokens, now: context.date)
                }
                Button("Open full details") { openDashboard(snapshot.id) }
                    .buttonStyle(SettingsButtonStyle(compact: true))
            }
        }
    }

    private var footer: some View {
        HStack(spacing: 14) {
            Button("Settings…", action: openSettings).buttonStyle(.plain)
            Button("Open Dashboard") { openDashboard(selectedID) }.buttonStyle(.plain)
            Spacer()
            Button { model.refresh(providerID: nil) } label: { Image(systemName: "arrow.clockwise") }
                .buttonStyle(.plain).disabled(!model.canRefresh || !model.analyticsLoading.isEmpty)
                .help("Refresh readings and local analytics")
            Button("Close", action: close).buttonStyle(.plain)
        }.font(.system(size: 11)).padding(14)
    }
}

/// Card chrome only. Observes hover so a pointer move does not rebuild the tabs.
private struct MenuBarProviderCard<Content: View>: View {
    @ObservedObject var hover: DetailHoverStore
    let snapshotID: String
    let accent: Color
    let glass: Bool
    private let content: Content

    init(hover: DetailHoverStore, snapshotID: String, accent: Color, glass: Bool,
         @ViewBuilder content: () -> Content) {
        self.hover = hover
        self.snapshotID = snapshotID
        self.accent = accent
        self.glass = glass
        self.content = content()
    }

    private var detail: DetailHoverState { hover.state }
    private var highlighted: Bool { detail.shownID == snapshotID || detail.hoveredID == snapshotID }

    var body: some View {
        content
            .padding(14)
            .background(MatraBrand.card.opacity(glass ? 0.55 : 1), in: RoundedRectangle(cornerRadius: 12))
            .background(highlighted ? accent.opacity(0.10) : Color.clear, in: RoundedRectangle(cornerRadius: 12))
            .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(
                highlighted ? accent.opacity(detail.pinnedID == snapshotID ? 0.9 : 0.55) : Color.clear,
                lineWidth: detail.pinnedID == snapshotID ? 1.5 : 1) }
            .contentShape(RoundedRectangle(cornerRadius: 12))
            .onHover { inside in
                if inside { hover.hoverCard(snapshotID) } else { hover.leaveCard(snapshotID) }
            }
            .onTapGesture { hover.activateCard(snapshotID) }
            .accessibilityElement(children: .contain)
            .accessibilityAddTraits(detail.pinnedID == snapshotID ? .isSelected : [])
            .accessibilityAction(named: detail.pinnedID == snapshotID ? "Hide usage details" : "Show usage details") {
                hover.activateCard(snapshotID)
            }
    }
}

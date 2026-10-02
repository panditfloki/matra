import SwiftUI

/// The existing Settings shell and hover content, given room to read.
/// This first slice intentionally has no invented cost totals or history.
struct DashboardView: View {
    @ObservedObject var model: DashboardModel
    @ObservedObject var preferences: Preferences
    let openSettings: () -> Void
    @Environment(\.matraReduceTransparency) private var reduceTransparency
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        HStack(spacing: 0) {
            sidebar
            VStack(alignment: .leading, spacing: 0) {
                header
                Divider().overlay(SettingsPalette.hairline)
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        if model.isLivePreview {
                            Label("Live preview · Real accounts, quotas and local logs, with separate preview settings. Keep the installed Mātrā quit while this runs.",
                                  systemImage: "dot.radiowaves.left.and.right")
                                .font(.system(size: 12))
                                .foregroundStyle(Palette.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        } else if model.readsLocalLogs {
                            Label("Local-log preview · Tokens, models, projects and costs are read offline from this Mac's Codex and Claude logs. Quotas and accounts are not polled here.",
                                  systemImage: "doc.text.magnifyingglass")
                                .font(.system(size: 12))
                                .foregroundStyle(Palette.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        } else if model.isPreview {
                            Label("Sample preview · No accounts are connected or refreshed here.",
                                  systemImage: "testtube.2")
                                .font(.system(size: 12))
                                .foregroundStyle(Palette.textSecondary)
                        }
                        if let snapshot = model.selectedSnapshot {
                            detail(snapshot)
                        } else if model.snapshots.isEmpty {
                            emptyState
                        } else {
                            overview
                        }
                    }
                    .padding(28)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .id(model.selectedID)
            }
        }
        .modifier(MatraWindowSurface(preferences: preferences))
        .environment(\.notchSurfaceStyle, preferences.notchSurfaceStyle)
        .environment(\.expandsUsageRows, true)
        .environment(\.tooltipSecondaryInk, TooltipGlassContrast.secondaryInk(
            surfaceStyle: preferences.notchSurfaceStyle, colorScheme: colorScheme,
            reduceTransparency: reduceTransparency))
        .environment(\.usageWatchLimit, preferences.watchLimit)
        .environment(\.usageCriticalLimit, preferences.criticalLimit)
        .environment(\.colorTransitionStyle, preferences.colorTransitionStyle)
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            Color.clear.frame(height: SettingsView.headerHeight)
            HStack(spacing: 10) {
                Image("MatraLogo").resizable().interpolation(.high).frame(width: 36, height: 36)
                VStack(alignment: .leading, spacing: 3) {
                    Text(MatraBrand.name).font(.system(size: 18, weight: .semibold))
                    Text("by dy/dx · fx").font(.system(size: 10, weight: .medium, design: .monospaced))
                        .foregroundStyle(Palette.textSecondary)
                }
            }
            .padding(.horizontal, 18).padding(.bottom, 24)

            sidebarButton("Overview", icon: "square.grid.2x2", selected: model.selectedID == nil) {
                model.select(nil)
            }
            .padding(.horizontal, 10)
            Text("AI SOURCES").font(.system(size: 10, weight: .semibold))
                .foregroundStyle(Palette.textSecondary)
                .padding(.horizontal, 20).padding(.top, 26).padding(.bottom, 10)
            ScrollView {
                VStack(spacing: 2) {
                    ForEach(model.snapshots) { snapshot in
                        Button { model.select(snapshot.id) } label: {
                            HStack(spacing: 10) {
                                ProviderGlyphView(glyph: snapshot.glyph,
                                                  customIconFilename: snapshot.customIconFilename, size: 16)
                                    .frame(width: 18)
                                Text(snapshot.localModel?.name ?? snapshot.displayName)
                                    .lineLimit(2).multilineTextAlignment(.leading)
                                Spacer(minLength: 0)
                            }
                            .font(.system(size: 13))
                            .padding(.horizontal, 10).padding(.vertical, 9)
                            .contentShape(Rectangle())
                            .background(selectionFill(model.selectedID == snapshot.id))
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(model.selectedID == snapshot.id ? .isSelected : [])
                    }
                }.padding(.horizontal, 10)
            }
            Spacer(minLength: 12)
            VStack(alignment: .leading, spacing: 8) {
                sidebarButton("Settings…", icon: "gearshape", selected: false, action: openSettings)
                Link("dydxfx.com", destination: MatraBrand.website)
                    .font(.system(size: 12, weight: .semibold)).foregroundStyle(MatraBrand.accent)
                    .padding(.horizontal, 10)
                Text(model.isLivePreview ? "Local test build · Live data" : model.readsLocalLogs ? "Local test build · Your local logs" : model.isPreview ? "Local test build · Sample data" : "Usage, in proportion.")
                    .font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
                    .padding(.horizontal, 10)
            }.padding(.horizontal, 10).padding(.bottom, 20)
        }
        .foregroundStyle(Palette.textPrimary)
        .frame(width: SettingsView.sidebarWidth)
        .background(SettingsPalette.sidebar.opacity(preferences.notchSurfaceStyle.isGlass && !reduceTransparency ? 0.60 : 1))
        .overlay(alignment: .trailing) { SettingsPalette.hairline.frame(width: 1) }
    }

    private func sidebarButton(_ title: String, icon: String, selected: Bool,
                               action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: icon)
                .font(.system(size: 13, weight: selected ? .semibold : .regular))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 10).padding(.vertical, 9)
                .contentShape(Rectangle()).background(selectionFill(selected))
        }.buttonStyle(.plain)
            .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func selectionFill(_ selected: Bool) -> some View {
        RoundedRectangle(cornerRadius: 8, style: .continuous)
            .fill(selected ? preferences.accentColor.color.opacity(0.14) : Color.clear)
    }

    private var header: some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 6) {
                Text(model.selectedSnapshot?.displayName ?? "Overview")
                    .font(.system(size: 24, weight: .semibold))
                Text(model.selectedID == nil ? "Your notch readings, with room to explore." : "The same reading as your side notch.")
                    .font(.system(size: 13)).foregroundStyle(Palette.textSecondary)
            }
            Spacer(minLength: 16)
            let busy = model.isRefreshingSelection || (model.readsLocalLogs && !model.analyticsLoading.isEmpty)
            Button(action: model.refresh) {
                Label(busy ? "Refreshing…" : model.readsLocalLogs ? "Refresh local logs" : "Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(SettingsButtonStyle())
            .disabled(!model.canRefresh || busy || model.snapshots.isEmpty)
            .keyboardShortcut("r", modifiers: .command)
        }
        .foregroundStyle(Palette.textPrimary)
        .padding(.horizontal, 28).padding(.vertical, 28)
    }

    private var overview: some View {
        VStack(alignment: .leading, spacing: 0) {
            let sources = Set(model.snapshots.map(DashboardModel.analyticsID))
            let reports = model.spendReports.values.filter { sources.contains($0.providerID) }
            if !reports.isEmpty {
                let total = UsageSpendReport.Value.sum(reports.map(\.total))
                Text("30-day local activity · \(SpendCopy.tokens(total)) tokens · \(SpendCopy.cost(total)) estimated API cost")
                    .font(.system(size: 16, weight: .semibold)).padding(.bottom, 12)
                Text("Distinct local sources only. Account quota and account-wide profile tokens are shown separately.")
                    .font(.system(size: 12)).foregroundStyle(Palette.textSecondary)
                    .padding(.bottom, SpendCopy.staleNote(Array(reports)) == nil ? 24 : 8)
                if let stale = SpendCopy.staleNote(Array(reports)) {
                    Label(stale, systemImage: "clock").font(.system(size: 12)).foregroundStyle(Palette.watch)
                        .padding(.bottom, 24)
                }
            }
            Text("Current allowances").font(.system(size: 16, weight: .semibold))
                .padding(.bottom, 8)
            Text("Each source has its own limits. These percentages are not added together.")
                .font(.system(size: 12)).foregroundStyle(Palette.textSecondary).padding(.bottom, 24)
            ForEach(model.snapshots) { snapshot in
                VStack(alignment: .leading, spacing: 16) {
                    TimelineView(.periodic(from: .now, by: 30)) { context in
                        ProviderTooltip(snapshot: snapshot, now: context.date,
                                        resetTimeFormat: preferences.resetTimeFormat,
                                        showUsagePace: preferences.showUsagePace)
                    }
                    if let report = model.report(for: snapshot) { UsageSpendSummary(report: report) }
                    HStack {
                        Text(sourceNote(snapshot)).font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
                        Spacer()
                        Button("Open details") { model.select(snapshot.id) }
                            .buttonStyle(SettingsButtonStyle(compact: true))
                    }
                }
                .padding(20)
                .background(MatraBrand.card.opacity(preferences.notchSurfaceStyle.isGlass ? 0.60 : 1),
                            in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                .padding(.bottom, 16)
            }
        }.foregroundStyle(Palette.textPrimary)
    }

    private func detail(_ snapshot: ProviderSnapshot) -> some View {
        TimelineView(.periodic(from: .now, by: 30)) { context in
            VStack(alignment: .leading, spacing: 24) {
                ProviderTooltip(snapshot: snapshot, now: context.date,
                                resetTimeFormat: preferences.resetTimeFormat,
                                showUsagePace: preferences.showUsagePace)
                if let credits = snapshot.availableResetCredits(at: context.date) {
                    UsageResetCreditsSection(credits: credits, now: context.date)
                }
                if let usage = snapshot.tokenUsage {
                    CodexUsageSection(usage: usage, now: context.date)
                } else if let history = snapshot.customUsageHistory {
                    CodexUsageSection(usage: history.codexUsage, now: context.date)
                }
                if let usage = snapshot.usageDetail, usage.hasUsage {
                    DeepSeekUsageDetail(detail: usage, now: context.date,
                        schedule: preferences.deepSeekPricingSchedule,
                        showsPricing: preferences.deepSeekPricingEnabled)
                }
                if let report = model.report(for: snapshot) {
                    Divider()
                    Text("Local device activity").font(.system(size: 16, weight: .semibold))
                    UsageSpendSummary(report: report)
                    UsageSpendView(report: report)
                    RecentUsageWindows(report: report, snapshot: snapshot)
                } else if let error = model.analyticsErrors[DashboardModel.analyticsID(snapshot)] {
                    Text(error).font(.system(size: 12)).foregroundStyle(Palette.textSecondary)
                }
                Divider()
                Text(sourceNote(snapshot)).font(.system(size: 12)).foregroundStyle(Palette.textSecondary)
                Text("Local log totals can include several accounts on this Mac. API-price estimates are not subscription bills. Account-wide and local token totals are not added together.")
                    .font(.system(size: 12)).foregroundStyle(Palette.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(24)
        .background(MatraBrand.card.opacity(preferences.notchSurfaceStyle.isGlass ? 0.60 : 1),
                    in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private func sourceNote(_ snapshot: ProviderSnapshot) -> String {
        if model.readsLocalLogs { return "Your local logs, read offline. Quota is not polled in this preview." }
        if model.isPreview { return "Sample data, not your account usage." }
        switch snapshot.fidelity {
        case .official: return "Provider-reported reading. Unavailable values stay unknown."
        case .derived: return "Derived reading, not a provider bill."
        case .manual: return "Manual reading, not a live provider measurement."
        }
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("No readings to show yet").font(.system(size: 20, weight: .semibold))
            Text("Manage your AI sources in Settings. Connected sources appear here when the shared usage store publishes a reading.")
                .font(.system(size: 13)).foregroundStyle(Palette.textSecondary)
            Button("Open Settings", action: openSettings).buttonStyle(SettingsButtonStyle())
        }.foregroundStyle(Palette.textPrimary).padding(.vertical, 36)
    }
}

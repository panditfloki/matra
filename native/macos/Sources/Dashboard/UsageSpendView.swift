import SwiftUI

enum SpendCopy {
    /// "~$x" only when every token is priced. Any unpriced token makes the
    /// figure "At least $x", and no priced token at all makes it unavailable.
    static func cost(_ value: UsageSpendReport.Value?) -> String {
        guard let value else { return "Unavailable" }
        guard let cost = value.costUSD else {
            guard value.tokens > 0 else { return "Unavailable" }
            // Codex prices a day, not each model: a priced model row has no own cost.
            return value.pricedTokens == value.tokens ? "Priced, not itemised" : "Cost unavailable"
        }
        return (value.incompleteCost ? "At least " : "~") + String(format: "$%.2f", cost)
    }
    /// A subtotal that leaves out boundary days and today is a lower bound.
    static func lowerBoundCost(_ value: UsageSpendReport.Value) -> String {
        guard let cost = value.costUSD else { return self.cost(value) }
        return "At least " + String(format: "$%.2f", cost)
    }
    /// Token-weighted, never day-counted: a day 1% priced is not "covered".
    static func coverage(_ value: UsageSpendReport.Value) -> String {
        guard value.tokens > 0 else { return "Cost coverage: no tokens recorded." }
        guard let priced = value.knownPricedTokens else { return "Cost coverage: unknown." }
        let share = Double(priced) / Double(value.tokens) * 100
        let text = priced > 0 && share < 1 ? "under 1%" : priced < value.tokens && share >= 99.5 ? "over 99%" : String(format: "%.0f%%", share)
        return "Cost coverage: \(text) of tokens priced." + (priced < value.tokens ? " Unpriced tokens add no cost here." : "")
    }
    /// Combined figures must say when a part is an older report.
    static func staleNote(_ reports: [UsageSpendReport]) -> String? {
        let names = reports.filter { $0.staleReason != nil }.map { $0.providerID.capitalized }.sorted()
        guard !names.isEmpty else { return nil }
        return "Includes an older \(names.joined(separator: " and ")) report: its last refresh failed."
    }
    static func tokens(_ value: UsageSpendReport.Value?) -> String {
        guard let value else { return "Pending" }
        return UsageFormat.tokens(value.tokens)
    }
}

/// Shared menu/dashboard chart and breakdown. Provider colours are stable;
/// app accents only style selections/actions, never change quota severity.
struct UsageSpendView: View {
    let report: UsageSpendReport
    var compact = false
    @State private var period: SpendPeriod = .month
    @State private var showsCost = false
    @State private var hoveredDay: String?

    private var days: [UsageSpendReport.Day] { report.days(in: period, now: Date()) }
    private var total: UsageSpendReport.Value { .sum(days.map(\.value)) }
    private var selectedDay: UsageSpendReport.Day? { days.first { $0.id == hoveredDay } }
    private var color: Color {
        switch report.providerID {
        case "codex": return Color(hex: 0x63BAC5)
        case "claude": return Color(hex: 0xD68A69)
        default: return Color(hex: 0x62B887)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Usage & Spend").font(.system(size: 14, weight: .semibold))
                Spacer()
                if report.staleReason != nil { Label("Stale", systemImage: "clock").font(.system(size: 11)) }
            }
            Picker("Reporting period", selection: $period) {
                ForEach(SpendPeriod.allCases) { Text($0.rawValue).tag($0) }
            }.pickerStyle(.menu).labelsHidden()
            HStack(alignment: .top) {
                metric("\(period.rawValue) tokens", value: SpendCopy.tokens(total))
                Spacer(minLength: 12)
                metric("Estimated API cost", value: SpendCopy.cost(total))
            }
            chart
            Picker("Chart measure", selection: $showsCost) {
                Text("Token").tag(false)
                Text("Cost").tag(true)
            }.pickerStyle(.segmented)
            if let day = selectedDay {
                Text("\(day.id): \(SpendCopy.tokens(day.value)) tokens · \(SpendCopy.cost(day.value))")
                    .font(.system(size: 11, weight: .medium)).fixedSize(horizontal: false, vertical: true)
            } else {
                Text("Hover a bar to inspect that day.").font(.system(size: 11)).foregroundStyle(Palette.textSecondary)
            }
            let rows = selectedDay?.models ?? UsageSpendReport.group(days.flatMap(\.models))
            if !rows.isEmpty {
                Text(selectedDay == nil ? "Models · selected period" : "Models · selected day")
                    .font(.system(size: 11, weight: .semibold)).foregroundStyle(Palette.textSecondary)
                ForEach(rows) { row in breakdown(row) }
            }
            if !report.projects.isEmpty {
                Divider()
                Text("Projects · loaded 30-day report")
                    .font(.system(size: 11, weight: .semibold)).foregroundStyle(Palette.textSecondary)
                ForEach(report.projects.prefix(compact ? 5 : 12)) { row in breakdown(row, project: true) }
                Text("Project totals use the session report. Model/day filters do not filter these rows.")
                    .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            }
            Text(report.pricingNote).font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            Text(SpendCopy.coverage(total))
                .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            Text("Local device logs · \(report.timeZoneID) · \(report.measuredAt.formatted(date: .omitted, time: .shortened))")
                .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            if let error = report.staleReason {
                Text(error).font(.system(size: 11)).foregroundStyle(Palette.watch)
            }
        }.foregroundStyle(Palette.textPrimary)
    }

    private var chart: some View {
        let maximum = days.map { showsCost ? ($0.value.costUSD ?? 0) : Double($0.value.tokens) }.max() ?? 0
        return VStack(alignment: .trailing, spacing: 5) {
            Text(showsCost && days.allSatisfy({ $0.value.costUSD == nil }) ? "Cost unavailable"
                 : showsCost ? String(format: "$%.2f", maximum) : UsageFormat.tokens(Int(maximum)))
                .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            HStack(alignment: .bottom, spacing: 3) {
                ForEach(days) { day in
                    let value = showsCost ? day.value.costUSD : Double(day.value.tokens)
                    RoundedRectangle(cornerRadius: 2)
                        .fill(value == nil ? Palette.barTrack : color.opacity(hoveredDay == day.id ? 1 : 0.70))
                        .frame(maxWidth: .infinity)
                        .frame(height: value.map { max(2, CGFloat($0 / max(1, maximum)) * (compact ? 100 : 140)) } ?? 4)
                        .frame(height: compact ? 104 : 144, alignment: .bottom)
                        .contentShape(Rectangle())
                        .onHover { if $0 { hoveredDay = day.id } }
                        .onTapGesture { hoveredDay = day.id }
                        .help("\(day.id): \(SpendCopy.tokens(day.value)) tokens · \(SpendCopy.cost(day.value))")
                        .accessibilityLabel("\(day.id), \(SpendCopy.tokens(day.value)) tokens, \(SpendCopy.cost(day.value)) estimated cost")
                }
            }
            .overlay(alignment: .bottom) { Palette.barTrack.frame(height: 1) }
            .accessibilityElement(children: .contain)
            HStack {
                Text(days.first?.id ?? "No data")
                Spacer()
                Text(days.last?.id ?? "")
            }.font(.system(size: 9)).foregroundStyle(Palette.textSecondary)
            if showsCost && days.contains(where: { $0.value.costUSD == nil }) {
                Text("Short neutral marks mean cost unavailable.").font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            }
            if showsCost && days.contains(where: { $0.value.costUSD != nil && $0.value.incompleteCost }) {
                Text("Partly priced days show only their priced part.").font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            }
        }
    }

    private func metric(_ title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title).font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            Text(value).font(.system(size: compact ? 18 : 24, weight: .semibold)).monospacedDigit()
        }
    }

    private func breakdown(_ row: UsageSpendReport.Breakdown, project: Bool = false) -> some View {
        HStack(alignment: .top, spacing: 9) {
            color.frame(width: 2)
            VStack(alignment: .leading, spacing: 3) {
                Text(project ? projectName(row.id) : row.id)
                    .font(.system(size: 11, weight: .medium)).lineLimit(2)
                Text("\(SpendCopy.tokens(row.value)) tokens · \(SpendCopy.cost(row.value))")
                    .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
                if project && row.id.hasPrefix("/") {
                    Text(row.id).font(.system(size: 9)).foregroundStyle(Palette.textSecondary).lineLimit(1).truncationMode(.middle)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }.fixedSize(horizontal: false, vertical: true)
    }
    private func projectName(_ path: String) -> String {
        path.hasPrefix("/") ? URL(fileURLWithPath: path).lastPathComponent : path
    }
}

struct UsageSpendSummary: View {
    let report: UsageSpendReport
    var body: some View {
        let today = report.today(now: Date())
        let total = report.total
        Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 12) {
            GridRow { metric("Today tokens", SpendCopy.tokens(today)); metric("Today estimated cost", SpendCopy.cost(today)) }
            GridRow { metric("30-day tokens", SpendCopy.tokens(total)); metric("30-day estimated cost", SpendCopy.cost(total)) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func metric(_ title: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
            Text(value).font(.system(size: 13, weight: .semibold)).monospacedDigit()
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct RecentUsageWindows: View {
    let report: UsageSpendReport
    let snapshot: ProviderSnapshot
    private var weekly: LimitWindow? {
        snapshot.weeklyWindow ?? snapshot.windows.first { ($0.duration ?? 0) >= 6 * 86400 }
    }
    var body: some View {
        if let window = weekly, let reset = window.resetsAt,
           let duration = window.duration, duration > 0 {
            VStack(alignment: .leading, spacing: 12) {
                Text("Recent windows").font(.system(size: 13, weight: .semibold))
                ForEach(0..<3) { index in
                    let end = reset.addingTimeInterval(-Double(index) * duration)
                    let start = end.addingTimeInterval(-duration)
                    let value = report.completeDaySubtotal(start: start, end: end, now: Date())
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(index == 0 ? "Current window" : index == 1 ? "Previous window" : "2 windows ago")
                            Spacer()
                            Text(value.map { "≥ \(UsageFormat.tokens($0.tokens)) tokens" } ?? "Unavailable")
                        }.font(.system(size: 11, weight: .medium))
                        Text("\(start.formatted(date: .abbreviated, time: .shortened)) to \(end.formatted(date: .abbreviated, time: .shortened))")
                            .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
                        if let value { Text(SpendCopy.lowerBoundCost(value)).font(.system(size: 11)) }
                    }
                }
                Text("Recorded complete days only. Boundary days and today are excluded; previous boundaries are inferred from this reset and duration.")
                    .font(.system(size: 10)).foregroundStyle(Palette.textSecondary).fixedSize(horizontal: false, vertical: true)
            }
        } else {
            Text("Recent windows need a published reset and window duration.")
                .font(.system(size: 10)).foregroundStyle(Palette.textSecondary)
        }
    }
}

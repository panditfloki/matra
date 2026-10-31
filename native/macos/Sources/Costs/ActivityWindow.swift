import SwiftUI
import AppKit
import Combine

/// Work over a day, week or month: which sessions ran, in which project, for how
/// long, and an estimate of what they cost. A lane chart or per-day bars, then
/// the detail. Built from indexed metadata only; transcripts are not opened here.
enum ActivityPeriod: String, CaseIterable, Identifiable {
    case day, week, month
    var id: String { rawValue }
    var title: String {
        switch self { case .day: return L10n.t("Day"); case .week: return L10n.t("Week"); case .month: return L10n.t("Month") }
    }

    /// The calendar day, week or month holding `date`.
    func interval(containing date: Date, calendar: Calendar) -> DateInterval {
        let component: Calendar.Component
        switch self {
        case .day: component = .day
        case .week: component = .weekOfYear
        case .month: component = .month
        }
        return calendar.dateInterval(of: component, for: date) ?? DateInterval(start: date, duration: 86_400)
    }
}

struct TimelinePane: View {
    @ObservedObject var accounts: CostAccountStore = .shared
    @ObservedObject var prices: PriceTable = .shared
    @ObservedObject var catalog: PlanCatalog = .shared
    @StateObject private var loader = ActivityReportLoader()
    @State private var day: Date = Calendar.current.startOfDay(for: Date())
    @State private var hovered: String?
    @State private var period: ActivityPeriod = .day

    private var cal: Calendar { Calendar.current }
    private var interval: DateInterval { period.interval(containing: day, calendar: cal) }
    private var report: ActivityReport { loader.scoped }

    private var isToday: Bool { interval.contains(Date()) }

    /// Account colours: the first account keeps Claude's orange, the rest get
    /// distinct hues so lanes read at a glance.
    static let accountColors: [Color] = [
        Color(red: 0.85, green: 0.47, blue: 0.34),
        Color(red: 0x2a/255, green: 0x9d/255, blue: 0x8f/255),
        Color(red: 0x7c/255, green: 0x6f/255, blue: 0xe0/255),
        Color(red: 0x2f/255, green: 0x80/255, blue: 0xed/255),
        Color(red: 0xd9/255, green: 0x53/255, blue: 0x8a/255),
    ]
    private func color(_ r: ActivitySession) -> Color { Self.accountColors[r.accountIndex % Self.accountColors.count] }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                header
                if report.sessions.isEmpty {
                    empty
                } else {
                    if period == .day { lanes } else { dayBars }
                    ForEach(report.projects) { g in projectBlock(g) }
                }
            }
            .padding(.leading, 56).padding(.trailing, 48).padding(.top, 30).padding(.bottom, 32)
            .frame(maxWidth: .infinity, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .onAppear(perform: load)
        .onChange(of: day) { load() }
        .onChange(of: period) { load() }
        .onChange(of: accounts.accounts) { load() }
        .onChange(of: prices.rate) { load() }
        .onChange(of: prices.prices) { load() }
        .onReceive(catalog.$plans.dropFirst()) { _ in load() }
    }

    // MARK: Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(L10n.t("Activity")).font(.system(size: 20, weight: .semibold))
                    Text(dayTitle).font(.system(size: 13)).foregroundColor(.secondary)
                }
                Spacer()
                HStack(spacing: 6) {
                    Button { shift(-1) } label: { Image(systemName: "chevron.left") }
                    Button(L10n.t("Today")) { day = Calendar.current.startOfDay(for: Date()) }.disabled(isToday)
                    Button { shift(1) } label: { Image(systemName: "chevron.right") }.disabled(isToday)
                    Button { load() } label: { Image(systemName: "arrow.clockwise") }
                }
                .controlSize(.small)
            }
            HStack(spacing: 12) {
                Picker("", selection: $period) { ForEach(ActivityPeriod.allCases) { Text($0.title).tag($0) } }
                    .pickerStyle(.segmented).labelsHidden().frame(width: 220)
                Picker("", selection: $loader.accountFilter) {
                    Text(L10n.t("All accounts")).tag("")
                    ForEach(accounts.accounts) { Text($0.name).tag($0.id) }
                }.labelsHidden().frame(width: 220)
                Picker("", selection: $loader.projectFilter) {
                    Text(L10n.t("All projects")).tag("")
                    ForEach(loader.report.projects) { Text($0.name).tag($0.project) }
                }.labelsHidden().frame(width: 220)
                Spacer()
            }
            HStack(spacing: 28) {
                stat(L10n.t("Sessions"), "\(report.sessions.count)")
                stat(L10n.t("Active time"), Self.duration(report.duration))
                stat(L10n.t("Turns"), "\(report.total.turns)")
                stat(L10n.t("Estimated spend"), report.total.estimatedSpend.text)
                Spacer()
                if accounts.accounts.count > 1 {
                    HStack(spacing: 10) {
                        ForEach(Array(accounts.accounts.enumerated()), id: \.element.id) { i, a in
                            HStack(spacing: 4) {
                                Circle().fill(Self.accountColors[i % Self.accountColors.count]).frame(width: 7, height: 7)
                                Text(a.name).font(.system(size: 11)).foregroundColor(.secondary)
                            }
                        }
                    }
                }
            }
            moneyBreakdown
        }
    }

    /// The kinds of money, kept apart and named for what they are.
    private var moneyBreakdown: some View {
        let total = report.total
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 28) {
                if total.planShare.coverage != .none { stat(L10n.t("Plan share (estimate)"), total.planShare.text) }
                if total.apiKeyUsage.coverage != .none { stat(L10n.t("API key usage (estimate)"), total.apiKeyUsage.text) }
                if total.apiEquivalent.coverage != .none { stat(L10n.t("API-equivalent (estimate)"), total.apiEquivalent.text) }
            }
            Text(L10n.t("Estimates in INR, not bills. Plan share allocates what the plan costs by the limit each turn used. API-equivalent is the same tokens at API list prices."))
                .font(.system(size: 11)).foregroundColor(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if !loader.report.slices.isEmpty, let note = coverageNote(total) {
                Text(note).font(.system(size: 11)).foregroundColor(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    /// Why a figure reads "At least" or "Unavailable", in numbers.
    private func coverageNote(_ total: CostTotals) -> String? {
        var parts: [String] = []
        let spend = total.estimatedSpend
        if spend.unknownTurns > 0 { parts.append(L10n.t("\(spend.unknownTurns) turns have no estimated spend.")) }
        let equivalent = total.apiEquivalent
        if !equivalent.unknownModels.isEmpty {
            parts.append(L10n.t("No list price for \(equivalent.unknownModels.sorted().map(Self.shortModel).joined(separator: ", "))."))
        } else if equivalent.unknownTurns > 0 {
            parts.append(L10n.t("\(equivalent.unknownTurns) turns have no API-equivalent yet."))
        }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }

    private func stat(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value).font(.system(size: 17, weight: .semibold)).monospacedDigit()
            Text(label).font(.system(size: 11)).foregroundColor(.secondary)
        }
    }

    private var empty: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(loader.loading ? L10n.t("Loading…") : L10n.t("No sessions in this period")).font(.system(size: 13, weight: .medium))
            if !loader.loading {
                Text(L10n.t("Turns are read from the transcripts each account writes; the first ones show up a few minutes after a session starts."))
                    .font(.system(size: 12)).foregroundColor(.secondary)
            }
        }
        .padding(.top, 8)
    }

    // MARK: Lane chart

    private var hourRange: (from: Int, to: Int) {
        let firsts = report.sessions.map { cal.component(.hour, from: $0.first) }
        let lasts = report.sessions.map { cal.component(.hour, from: $0.last) }
        let from = max(0, (firsts.min() ?? 8) - 1)
        let to = min(24, (lasts.max() ?? 18) + 2)
        return (from, max(to, from + 4))
    }

    private var lanes: some View {
        let range = hourRange
        let hours = Array(range.from...range.to)
        return VStack(alignment: .leading, spacing: 0) {
            // Hour axis.
            GeometryReader { geo in
                let w = geo.size.width - 150
                ZStack(alignment: .topLeading) {
                    ForEach(hours, id: \.self) { h in
                        let x = 150 + w * CGFloat(h - range.from) / CGFloat(range.to - range.from)
                        Text(String(format: "%02d", h))
                            .font(.system(size: 10)).monospacedDigit().foregroundColor(.secondary)
                            .position(x: x, y: 8)
                    }
                }
            }
            .frame(height: 18)
            ForEach(report.projects) { g in
                HStack(spacing: 0) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(g.name).font(.system(size: 12, weight: .medium)).lineLimit(1)
                        Text(Self.duration(g.duration)).font(.system(size: 10)).foregroundColor(.secondary)
                    }
                    .frame(width: 140, alignment: .leading)
                    .padding(.trailing, 10)
                    GeometryReader { geo in
                        let w = geo.size.width
                        ZStack(alignment: .leading) {
                            // Hour grid.
                            ForEach(hours, id: \.self) { h in
                                let x = w * CGFloat(h - range.from) / CGFloat(range.to - range.from)
                                Rectangle().fill(Color.primary.opacity(0.06)).frame(width: 1).offset(x: x)
                            }
                            ForEach(g.sessions) { r in
                                let x0 = w * fraction(r.first, range)
                                let x1 = w * fraction(r.last, range)
                                RoundedRectangle(cornerRadius: 3, style: .continuous)
                                    .fill(color(r).opacity(hovered == nil || hovered == r.id ? 0.9 : 0.35))
                                    .frame(width: max(4, x1 - x0), height: 14)
                                    .offset(x: x0)
                                    .help("\(r.title) · \(Self.time(r.first))–\(Self.time(r.last))")
                                    .onHover { hovered = $0 ? r.id : nil }
                            }
                        }
                    }
                    .frame(height: 22)
                }
                .padding(.vertical, 5)
                Divider()
            }
        }
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(Color.primary.opacity(0.08)))
    }

    private func fraction(_ d: Date, _ range: (from: Int, to: Int)) -> CGFloat {
        let secs = d.timeIntervalSince(interval.start)
        let f = (secs / 3600 - Double(range.from)) / Double(range.to - range.from)
        return CGFloat(min(max(f, 0), 1))
    }

    // MARK: Detail

    private func projectBlock(_ g: ActivityProject) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text(g.name).font(.system(size: 13, weight: .semibold))
                    .help(g.project)
                Text(Self.duration(g.duration)).font(.system(size: 12)).foregroundColor(.secondary)
                Spacer()
                Text(g.totals.estimatedSpend.text)
                    .font(.system(size: 13, weight: .semibold)).monospacedDigit()
            }
            .padding(.horizontal, 2)
            VStack(spacing: 0) {
                ForEach(Array(g.sessions.enumerated()), id: \.element.id) { i, r in
                    sessionRow(r)
                    if i < g.sessions.count - 1 { Divider().padding(.leading, 12) }
                }
            }
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color(nsColor: .controlBackgroundColor)))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(Color.primary.opacity(0.08)))
        }
    }

    private func sessionRow(_ r: ActivitySession) -> some View {
        HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 2).fill(color(r)).frame(width: 3, height: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text("\(Self.time(r.first)) – \(Self.time(r.last))")
                    .font(.system(size: 12, weight: .medium)).monospacedDigit()
                Text(Self.duration(r.duration)).font(.system(size: 10.5)).foregroundColor(.secondary)
            }
            .frame(width: 110, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                Text(r.title).font(.system(size: 12)).lineLimit(1).help(r.cwd)
                HStack(spacing: 6) {
                    if accounts.accounts.count > 1 {
                        Text(r.accountName).font(.system(size: 10, weight: .semibold))
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .background(Capsule().fill(color(r).opacity(0.15)))
                    }
                    Text(r.models.sorted().map(Self.shortModel).joined(separator: ", "))
                        .font(.system(size: 10.5)).foregroundColor(.secondary).lineLimit(1)
                    Text("·").foregroundColor(.secondary)
                    Text(L10n.t("\(r.totals.turns) turns")).font(.system(size: 10.5)).foregroundColor(.secondary)
                    Text("·").foregroundColor(.secondary)
                    Text(MoneyFormat.tokens(r.totals.tokens) + " tok").font(.system(size: 10.5)).foregroundColor(.secondary)
                }
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                Text(r.totals.estimatedSpend.text)
                    .font(.system(size: 12, weight: .medium)).monospacedDigit().lineLimit(1)
                Text(r.billing == .subscription
                     ? L10n.t("Plan share (estimate)") : L10n.t("API key usage (estimate)"))
                    .font(.system(size: 10)).foregroundColor(.secondary).lineLimit(1)
                if r.billing == .subscription {
                    Text(L10n.t("API-equivalent \(r.totals.apiEquivalent.text)"))
                        .font(.system(size: 10)).foregroundColor(.secondary).monospacedDigit().lineLimit(1)
                }
            }
            .frame(minWidth: 130, alignment: .trailing)
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(hovered == r.id ? Color.primary.opacity(0.04) : .clear)
        .onHover { hovered = $0 ? r.id : nil }
    }

    // MARK: Data

    private func shift(_ n: Int) {
        let comp: Calendar.Component = period == .day ? .day : (period == .week ? .weekOfYear : .month)
        day = cal.date(byAdding: comp, value: n, to: day) ?? day
    }

    private var dayTitle: String {
        let f = DateFormatter()
        f.locale = L10n.locale
        switch period {
        case .day: f.dateStyle = .full; f.timeStyle = .none; return f.string(from: day).capitalized
        case .week:
            f.dateFormat = "d MMM"
            return "\(f.string(from: interval.start)) – \(f.string(from: interval.end.addingTimeInterval(-1)))"
        case .month: f.dateFormat = "LLLL yyyy"; return f.string(from: day).capitalized
        }
    }

    // MARK: Bars per day (week / month)

    @State private var hoveredDay: Date?

    static func dayTitle(_ d: Date) -> String {
        let f = DateFormatter(); f.locale = L10n.locale; f.dateFormat = "EEE d MMM"
        return f.string(from: d).capitalized
    }

    private var dayBars: some View {
        var d = interval.start
        var days: [Date] = []
        while d < interval.end { days.append(d); d = cal.date(byAdding: .day, value: 1, to: d)! }
        let perDay = Dictionary(report.days.map { ($0.day, $0) }, uniquingKeysWith: { a, _ in a })
        let maxV = max(report.days.map(\.totals.estimatedSpend.amount).max() ?? 0, 0.01)
        let f = DateFormatter(); f.dateFormat = period == .week ? "EEE" : "d"
        f.locale = L10n.locale
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .bottom, spacing: period == .week ? 14 : 4) {
                ForEach(days, id: \.self) { dd in
                    let spend = perDay[dd]?.totals.estimatedSpend ?? CostValue()
                    VStack(spacing: 4) {
                        // A month of bars has no room for a label each: the
                        // hovered bar reads out in the caption instead.
                        Text(period == .week && spend.coverage != .none ? spend.text : " ")
                            .font(.system(size: 9)).foregroundColor(.secondary).monospacedDigit().lineLimit(1)
                        RoundedRectangle(cornerRadius: 3, style: .continuous)
                            .fill(cal.isDateInToday(dd) || hoveredDay == dd ? Color(red: 0.85, green: 0.47, blue: 0.34) : Color(red: 0.85, green: 0.47, blue: 0.34).opacity(0.55))
                            .frame(height: max(3, 120 * CGFloat(spend.amount / maxV)))
                            .frame(maxWidth: .infinity)
                            .contentShape(Rectangle())
                            .onHover { hoveredDay = $0 ? dd : (hoveredDay == dd ? nil : hoveredDay) }
                        Text(f.string(from: dd).capitalized)
                            .font(.system(size: 10, weight: cal.isDateInToday(dd) ? .semibold : .regular))
                            .foregroundColor(cal.isDateInToday(dd) ? .primary : .secondary)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .frame(height: 160)
            HStack {
                if let dd = hoveredDay {
                    Text("\(Self.dayTitle(dd)) · \((perDay[dd]?.totals.estimatedSpend ?? CostValue()).text) · \(Self.duration(perDay[dd]?.duration ?? 0))")
                        .font(.system(size: 11, weight: .medium)).monospacedDigit()
                } else {
                    Text(L10n.t("Estimated spend per day; hover a bar for active time")).font(.system(size: 10.5)).foregroundColor(.secondary)
                }
                Spacer()
                Text("\(L10n.t("Total")) \(report.total.estimatedSpend.text) · \(Self.duration(report.duration))")
                    .font(.system(size: 11, weight: .medium)).monospacedDigit().foregroundColor(.secondary)
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(Color.primary.opacity(0.08)))
    }

    private func load() {
        let interval = self.interval
        let calendar = cal
        let pricer = prices.pricer
        let allAccounts = accounts.accounts
        let sources = CostModels.all.compactMap { m -> ActivitySource? in
            guard let store = m.store_, let index = allAccounts.firstIndex(where: { $0.id == m.id }) else { return nil }
            let account = m.account
            return ActivitySource(accountID: account.id, accountName: account.name, accountIndex: index,
                                  plan: CostPlan(account: account, pricer: pricer), store: store)
        }
        loader.load { ActivityAccounting.report(sources: sources, interval: interval, calendar: calendar) }
    }

    static func time(_ d: Date) -> String {
        let f = DateFormatter(); f.dateFormat = "HH:mm"; return f.string(from: d)
    }
    static func duration(_ s: TimeInterval) -> String {
        let m = Int(s / 60)
        if m < 60 { return L10n.t("\(m) min") }
        return m % 60 == 0 ? L10n.t("\(m / 60) h") : L10n.t("\(m / 60) h \(m % 60) min")
    }
    static func shortModel(_ m: String) -> String {
        m.replacingOccurrences(of: "claude-", with: "").replacingOccurrences(of: "[1m]", with: "").replacingOccurrences(of: "openai/", with: "")
    }
}

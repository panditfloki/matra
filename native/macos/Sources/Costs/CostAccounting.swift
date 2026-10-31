import Foundation
import Combine

/// What the cost layer may touch in this process.
///
/// A test run hosts the app, and a sample or local-log preview promises that
/// accounts stay untouched, so in those the singletons are inert: no profile
/// discovery, no reads or writes in Application Support, no network, no timers.
/// A live preview is a real run under its own storage identity.
enum CostRuntime {
    static let isInert = isInert(underTest: Runtime.isUnderTest,
                                 designPreview: Runtime.isDesignPreview,
                                 livePreview: Runtime.isLivePreview)

    static func isInert(underTest: Bool, designPreview: Bool, livePreview: Bool) -> Bool {
        underTest || (designPreview && !livePreview)
    }

    /// The one folder this layer owns; separate for a preview identity.
    static var directory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return MatraStorage.supportRoot(in: base).appendingPathComponent("costs", isDirectory: true)
    }
}

/// Every amount this layer shows is Indian rupees. The currency does not follow
/// the Mac's region, so nothing can be converted to one currency and labelled
/// another.
enum CostCurrency {
    static let code = "INR"
}

// MARK: - Money that knows how complete it is

/// A rupee sum plus what it could not price. `amount` alone is never the answer:
/// a total with unpriced turns is a minimum, and one with nothing priced is
/// unknown, which must not read as ₹0.
struct CostValue: Equatable, Sendable {
    enum Coverage: Equatable, Sendable {
        case none          // nothing to report
        case complete      // every turn priced; zero here is a true zero
        case atLeast       // some priced, some not: `amount` is a lower bound
        case unavailable   // none priced
    }

    private(set) var amount: Double = 0
    private(set) var pricedTurns = 0
    private(set) var unknownTurns = 0
    private(set) var unknownTokens = 0
    private(set) var unknownModels: Set<String> = []

    init() {}

    static func known(_ amount: Double) -> CostValue {
        var value = CostValue()
        value.add(amount)
        return value
    }

    static var unavailable: CostValue {
        var value = CostValue()
        value.add(nil)
        return value
    }

    /// Records one turn's cost; nil, or a number that is not a real amount, is
    /// counted as unknown instead of joining the sum.
    mutating func add(_ cost: Double?, tokens: Int = 0, model: String? = nil) {
        if let cost, cost.isFinite, cost >= 0 {
            amount += cost
            pricedTurns += 1
        } else {
            unknownTurns += 1
            unknownTokens += tokens
            if let model, !model.isEmpty { unknownModels.insert(model) }
        }
    }

    static func + (a: CostValue, b: CostValue) -> CostValue {
        var out = a
        out.amount += b.amount
        out.pricedTurns += b.pricedTurns
        out.unknownTurns += b.unknownTurns
        out.unknownTokens += b.unknownTokens
        out.unknownModels.formUnion(b.unknownModels)
        return out
    }

    var coverage: Coverage {
        if unknownTurns == 0 { return pricedTurns > 0 ? .complete : .none }
        return pricedTurns > 0 ? .atLeast : .unavailable
    }

    var text: String {
        switch coverage {
        case .none:        return "-"
        case .complete:    return MoneyFormat.inr(amount)
        case .atLeast:     return L10n.t("At least \(MoneyFormat.inr(amount))")
        case .unavailable: return L10n.t("Unavailable")
        }
    }
}

/// One range's money, by what kind of number each is. All three are estimates
/// and none is a bill; the labels keep them apart.
struct CostTotals: Equatable, Sendable {
    /// Subscription accounts: a slice of what the plan costs.
    var planShare = CostValue()
    /// API-key accounts: the tokens at list price, which is what they are metered on.
    var apiKeyUsage = CostValue()
    /// Every account: what the same tokens would cost at API list price. For a
    /// subscription this is a comparison, not money that was spent.
    var apiEquivalent = CostValue()
    var tokens = 0
    var turns = 0

    /// What the account is estimated to have cost, whichever way it pays.
    var estimatedSpend: CostValue { planShare + apiKeyUsage }

    mutating func add(_ turn: UsageTurn, estimator: CostEstimator) {
        tokens += turn.tokens
        turns += 1
        let list = estimator.pricer.local(model: turn.model, input: turn.input, output: turn.output,
                                          cacheRead: turn.cacheRead, cacheWrite: turn.cacheWrite)
        apiEquivalent.add(list, tokens: turn.tokens, model: turn.model)
        switch estimator.billing {
        case .subscription:
            planShare.add(estimator.planShare(at: turn.ts, weight: turn.weight), tokens: turn.tokens)
        case .api:
            apiKeyUsage.add(list, tokens: turn.tokens, model: turn.model)
        }
    }

    static func + (a: CostTotals, b: CostTotals) -> CostTotals {
        var out = a
        out.planShare = a.planShare + b.planShare
        out.apiKeyUsage = a.apiKeyUsage + b.apiKeyUsage
        out.apiEquivalent = a.apiEquivalent + b.apiEquivalent
        out.tokens += b.tokens
        out.turns += b.turns
        return out
    }
}

// MARK: - The activity report

/// One account's work in one project, in one session, on one day: the smallest
/// piece the report keeps. Sessions, projects, accounts and days are all sums of
/// these and nothing else, which is why their totals have to agree.
struct ActivitySlice: Equatable, Sendable {
    var accountID: String
    var accountName: String
    var accountIndex: Int
    var billing: CostAccount.Billing
    var sessionID: String
    var project: String
    var cwd: String
    var branch: String?
    var day: Date
    var first: Int
    var last: Int
    var models: Set<String>
    var totals = CostTotals()

    var duration: TimeInterval { max(120, TimeInterval(last - first)) }
}

/// One row of the detail list: only metadata, never transcript text.
struct ActivitySession: Identifiable, Equatable, Sendable {
    var accountID: String
    var accountName: String
    var accountIndex: Int
    var billing: CostAccount.Billing
    var sessionID: String
    var project: String
    var cwd: String
    var branch: String?
    var first: Date
    var last: Date
    var models: Set<String>
    var totals: CostTotals
    var duration: TimeInterval

    var id: String { accountID + "\u{0}" + sessionID + "\u{0}" + project }
    var shortID: String { String(sessionID.prefix(8)) }
    var title: String { branch.map { "\($0) · \(shortID)" } ?? shortID }
}

struct ActivityProject: Identifiable, Equatable, Sendable {
    var project: String
    var sessions: [ActivitySession]
    var totals: CostTotals
    var duration: TimeInterval
    var id: String { project }
    var name: String { ProjectCost(project: project, pct: 0).displayName }
}

struct ActivityAccount: Identifiable, Equatable, Sendable {
    var accountID: String
    var accountName: String
    var totals: CostTotals
    var duration: TimeInterval
    var id: String { accountID }
}

struct ActivityDay: Identifiable, Equatable, Sendable {
    var day: Date
    var totals: CostTotals
    var duration: TimeInterval
    var id: Date { day }
}

struct ActivityReport: Equatable, Sendable {
    let slices: [ActivitySlice]
    let total: CostTotals
    let duration: TimeInterval
    let sessions: [ActivitySession]
    let projects: [ActivityProject]
    let accounts: [ActivityAccount]
    let days: [ActivityDay]

    static let empty = ActivityReport(slices: [])

    init(slices: [ActivitySlice]) {
        self.slices = slices
        var total = CostTotals()
        var duration: TimeInterval = 0
        var bySession: [String: ActivitySession] = [:]
        var byAccount: [String: ActivityAccount] = [:]
        var byDay: [Date: ActivityDay] = [:]

        for s in slices {
            total = total + s.totals
            duration += s.duration

            let first = Date(timeIntervalSince1970: TimeInterval(s.first))
            let last = Date(timeIntervalSince1970: TimeInterval(s.last))
            let key = s.accountID + "\u{0}" + s.sessionID + "\u{0}" + s.project
            if var session = bySession[key] {
                session.first = min(session.first, first)
                session.last = max(session.last, last)
                session.models.formUnion(s.models)
                session.branch = s.branch ?? session.branch
                session.totals = session.totals + s.totals
                session.duration += s.duration
                bySession[key] = session
            } else {
                bySession[key] = ActivitySession(accountID: s.accountID, accountName: s.accountName,
                                                 accountIndex: s.accountIndex, billing: s.billing,
                                                 sessionID: s.sessionID, project: s.project, cwd: s.cwd,
                                                 branch: s.branch, first: first, last: last, models: s.models,
                                                 totals: s.totals, duration: s.duration)
            }

            var account = byAccount[s.accountID]
                ?? ActivityAccount(accountID: s.accountID, accountName: s.accountName, totals: CostTotals(), duration: 0)
            account.totals = account.totals + s.totals
            account.duration += s.duration
            byAccount[s.accountID] = account

            var day = byDay[s.day] ?? ActivityDay(day: s.day, totals: CostTotals(), duration: 0)
            day.totals = day.totals + s.totals
            day.duration += s.duration
            byDay[s.day] = day
        }

        let sessions = bySession.values.sorted { ($0.first, $0.id) < ($1.first, $1.id) }
        var projects: [ActivityProject] = []
        for (project, list) in Dictionary(grouping: sessions, by: \.project) {
            projects.append(ActivityProject(project: project, sessions: list,
                                            totals: list.reduce(CostTotals()) { $0 + $1.totals },
                                            duration: list.reduce(0) { $0 + $1.duration }))
        }
        projects.sort { a, b in
            let (x, y) = (a.totals.estimatedSpend.amount, b.totals.estimatedSpend.amount)
            if x != y { return x > y }
            if a.duration != b.duration { return a.duration > b.duration }
            return a.project < b.project
        }

        self.total = total
        self.duration = duration
        self.sessions = sessions
        self.projects = projects
        self.accounts = byAccount.values.sorted { $0.accountID < $1.accountID }
        self.days = byDay.values.sorted { $0.day < $1.day }
    }

    /// The same report for one account and/or project; "" keeps everything.
    func scoped(accountID: String = "", project: String = "") -> ActivityReport {
        guard !accountID.isEmpty || !project.isEmpty else { return self }
        return ActivityReport(slices: slices.filter {
            (accountID.isEmpty || $0.accountID == accountID) && (project.isEmpty || $0.project == project)
        })
    }
}

// MARK: - Building it

/// What an account costs, read on the main actor and carried off it.
struct CostPlan: Sendable {
    var billing: CostAccount.Billing
    var monthlyINR: Double?
    var creditPointINR: Double?
    var pricer: Pricer

    @MainActor init(account: CostAccount, pricer: Pricer) {
        billing = account.billing
        monthlyINR = account.monthlyINR(rate: pricer.rate)
        creditPointINR = account.creditPointINR(rate: pricer.rate)
        self.pricer = pricer
    }

    init(billing: CostAccount.Billing, monthlyINR: Double?, creditPointINR: Double? = nil, pricer: Pricer) {
        self.billing = billing
        self.monthlyINR = monthlyINR
        self.creditPointINR = creditPointINR
        self.pricer = pricer
    }
}

/// One account's data going into a report.
struct ActivitySource: Sendable {
    var accountID: String
    var accountName: String
    var accountIndex: Int
    var plan: CostPlan
    var store: CostStore
}

enum ActivityAccounting {
    /// The estimator for an account's turns in [from, to]. The limit window is
    /// whichever allowance the account last reported; with none ever reported,
    /// each calendar month's price is spread over that month's tokens.
    static func estimator(plan: CostPlan, store: CostStore, from: Int, to: Int, calendar: Calendar) -> CostEstimator {
        guard plan.billing == .subscription else {
            return CostEstimator(billing: plan.billing, monthlyPrice: plan.monthlyINR, periods: [], pricer: plan.pricer)
        }
        let allowance = store.activeAllowance()
        let periods = allowance.map { store.allocationPeriods(window: $0) } ?? []
        return CostEstimator(billing: plan.billing, monthlyPrice: plan.monthlyINR,
                             creditPointValue: allowance == .credits ? plan.creditPointINR : nil,
                             creditBacked: allowance == .credits,
                             periods: periods,
                             monthWeights: periods.isEmpty ? store.monthWeights(from: from, to: to, calendar: calendar) : [:],
                             calendar: calendar,
                             pricer: plan.pricer)
    }

    /// Every source's turns in `interval`, each priced on its own.
    static func report(sources: [ActivitySource], interval: DateInterval,
                       calendar: Calendar = .current) -> ActivityReport {
        let from = Int(interval.start.timeIntervalSince1970)
        let to = Int(interval.end.timeIntervalSince1970) - 1
        var slices: [ActivitySlice] = []
        for source in sources {
            let estimator = Self.estimator(plan: source.plan, store: source.store, from: from, to: to, calendar: calendar)
            var bySlice: [String: ActivitySlice] = [:]
            // Turns arrive oldest first, so the day only needs working out when it changes.
            var today: DateInterval?
            for turn in source.store.turns(from: from, to: to) {
                let date = Date(timeIntervalSince1970: TimeInterval(turn.ts))
                if today?.contains(date) != true || date == today?.end {
                    today = calendar.dateInterval(of: .day, for: date)
                }
                let day = today?.start ?? calendar.startOfDay(for: date)
                let key = "\(turn.sessionID)\u{0}\(turn.projectKey)\u{0}\(day.timeIntervalSince1970)"
                var slice = bySlice[key]
                    ?? ActivitySlice(accountID: source.accountID, accountName: source.accountName,
                                     accountIndex: source.accountIndex, billing: source.plan.billing,
                                     sessionID: turn.sessionID, project: turn.projectKey, cwd: turn.cwd,
                                     branch: turn.branch, day: day, first: turn.ts, last: turn.ts, models: [])
                slice.first = min(slice.first, turn.ts)
                slice.last = max(slice.last, turn.ts)
                slice.branch = turn.branch ?? slice.branch
                slice.models.insert(turn.model)
                slice.totals.add(turn, estimator: estimator)
                bySlice[key] = slice
            }
            slices.append(contentsOf: bySlice.values)
        }
        return ActivityReport(slices: slices)
    }
}

/// Loads reports off the main thread. The newest request wins: changing the date
/// quickly starts several loads, and they can finish in any order, so an answer
/// to anything but the latest question is thrown away.
@MainActor
final class ActivityReportLoader: ObservableObject {
    @Published private(set) var report = ActivityReport.empty
    @Published private(set) var scoped = ActivityReport.empty
    @Published private(set) var loading = false
    @Published var accountFilter = "" { didSet { rescope() } }
    @Published var projectFilter = "" { didSet { rescope() } }

    private var generation = 0
    private var task: Task<Void, Never>?

    func load(_ work: @escaping @Sendable () -> ActivityReport) {
        generation += 1
        let request = generation
        task?.cancel()
        loading = true
        task = Task.detached(priority: .userInitiated) { [weak self] in
            let answer = work()
            await self?.finish(answer, request: request)
        }
    }

    private func finish(_ answer: ActivityReport, request: Int) {
        guard request == generation else { return }
        report = answer
        if !projectFilter.isEmpty, !answer.projects.contains(where: { $0.project == projectFilter }) {
            projectFilter = ""
        }
        rescope()
        loading = false
    }

    private func rescope() {
        scoped = report.scoped(accountID: accountFilter, project: projectFilter)
    }
}

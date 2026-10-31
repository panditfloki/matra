import Foundation
import SwiftUI
import Combine

/// Drives the "what used it" section of the Claude card.
///
/// The limit itself only tells you how much is gone. This pairs each observed
/// increase with the local turns that happened in the same interval, so the card
/// can say *which project* spent it. Everything is computed locally from files
/// Claude Code already wrote; no extra requests, no accounts.
@MainActor
final class CostModel: ObservableObject {

    let id: String
    private let initialAccount: CostAccount
    /// Settings can change how the account is paid at any time, so read it fresh.
    var account: CostAccount { CostAccountStore.shared.account(id) ?? initialAccount }

    enum State {
        case unavailable      // no transcripts on this machine: hide the section
        case waiting          // indexed, but no measurable consumption yet
        case ready
    }

    @Published private(set) var rows: [ProjectCost] = []
    /// Tokens per day and the summary the card's chart reads, from the
    /// transcripts; Codex brings its own from the server.
    @Published private(set) var tokenUsage: CodexTokenUsage?
    @Published private(set) var state: State = .unavailable
    /// False for plans with no rolling limits (credit-based seats): the session
    /// and week views need limit samples, so only month and all-time apply.
    @Published private(set) var quotaBacked = true
    /// The allowance the "week" tab measures: the weekly limit, or the credit
    /// cycle for credit-based seats.
    @Published private(set) var allowance: CostWindow = .weekly
    var creditBacked: Bool { allowance == .credits }

    @Published var isExpanded: Bool {
        didSet {
            guard !CostRuntime.isInert else { return }
            UserDefaults.standard.set(isExpanded, forKey: Self.expandedKey)
        }
    }

    /// Which range the list is showing. Persisted, like the expanded state.
    @Published var range: CostRange {
        didSet {
            if !CostRuntime.isInert { UserDefaults.standard.set(range.rawValue, forKey: Self.rangeKey) }
            reload()
        }
    }

    private static let expandedKey = "costExpanded"
    private static let rangeKey = "costRange"
    private let store: CostStore?
    private let indexer: CostIndexer?
    private var reloadRequest = 0

    /// The real one: one database and one transcript watcher per account. Inert
    /// in a test run or a sample preview, where neither may exist.
    convenience init(account: CostAccount) {
        guard !CostRuntime.isInert else {
            self.init(account: account, store: nil, indexer: nil)
            return
        }
        // The original single-account database keeps its name for the default
        // Claude login; every other account (Codex included) gets its own file.
        let store = CostStore(url: CostRuntime.directory.appendingPathComponent("agentcost-\(account.id).sqlite"))
        let indexer = store.flatMap {
            CostIndexer(store: $0, root: account.transcriptsRoot, format: account.provider == "codex" ? .codex : .claude)
        }
        self.init(account: account, store: store, indexer: indexer)
        guard indexer != nil else { return }
        state = .waiting
        indexer?.onChange = { [weak self] in
            Task { @MainActor in self?.reload() }
        }
        indexer?.start()
        reload()
    }

    /// For tests: a store the caller owns, with nothing watching the disk.
    convenience init(account: CostAccount, store: CostStore) {
        self.init(account: account, store: store, indexer: nil)
        state = .waiting
    }

    private init(account: CostAccount, store: CostStore?, indexer: CostIndexer?) {
        id = account.id
        initialAccount = account
        isExpanded = UserDefaults.standard.bool(forKey: Self.expandedKey)
        let saved = CostRange(rawValue: UserDefaults.standard.string(forKey: Self.rangeKey) ?? "") ?? .session
        range = saved == .allTime ? .month : saved
        self.store = store
        self.indexer = indexer
    }

    /// Called after every usage poll. Samples the limit and attributes any increase.
    /// Matra's windows carry ids: "session"/"primary" are the rolling
    /// session, "weekly_all"/"secondary" the week, anything named credits the
    /// credit cap of a Business seat.
    func observe(_ snapshots: [ProviderSnapshot]) {
        guard let store, let snap = snapshots.first(where: { $0.id == id }) else { return }
        let samples: [(window: CostWindow, pct: Double, resetsAt: Date?)] = snap.windows.compactMap { w in
            guard let f = w.usedFraction else { return nil }
            let window: CostWindow
            switch w.id {
            case "session", "primary": window = .session
            case "weekly_all", "weekly", "secondary": window = .weekly
            default:
                if w.id.lowercased().contains("credit") || w.label.lowercased().contains("credit") { window = .credits } else { return nil }
            }
            if window == .credits, let used = w.used, let remaining = w.remaining {
                CostAccountStore.shared.setCreditLimit(id, Double(used + remaining))
            }
            return (window, f * 100, w.resetsAt)
        }
        if samples.isEmpty {
            if quotaBacked { quotaBacked = false; if range.window != nil { range = .month } }
            return
        }
        if !quotaBacked { quotaBacked = true }
        let win: CostWindow = samples.contains { $0.window == .credits } ? .credits : .weekly
        if allowance != win { allowance = win; if range == .session && win == .credits { range = .weekly } }
        CostAccountStore.shared.detectPlanIfDue(id)

        Task.detached(priority: .utility) {
            for s in samples {
                store.recordSample(window: s.window, pct: s.pct, resetsAt: s.resetsAt)
            }
            await MainActor.run { self.reload() }
        }
    }

    func reload() {
        guard let store else { return }
        let range = (quotaBacked || self.range.window == nil) ? self.range : .month
        let plan = CostPlan(account: account, pricer: PriceTable.shared.pricer)
        let quotaBacked = self.quotaBacked
        let allowance = self.allowance
        // Ranges can change faster than this answers: only the newest may land.
        reloadRequest += 1
        let request = reloadRequest
        Task.detached(priority: .utility) {
            let done = Self.computeRows(range: range, allowance: allowance, quotaBacked: quotaBacked,
                                        plan: plan, store: store).presentable()
            let usage = store.tokenUsage()
            await MainActor.run {
                guard request == self.reloadRequest else { return }
                self.rows = done
                self.state = done.isEmpty ? .waiting : .ready
                if self.tokenUsage != usage { self.tokenUsage = usage }
            }
        }
    }

    /// The card's rows for a range, each with its estimated money.
    ///
    /// Inside the allowance window every row's share is already a share of that
    /// window, so its money is that share of the plan: the same rule for every
    /// row, whether its share came from a recorded jump or from the gap spread
    /// over the tokens. Every other range prices its turns one by one, exactly as
    /// the Activity window does, so the two surfaces agree.
    nonisolated static func computeRows(range: CostRange, allowance: CostWindow, quotaBacked: Bool, plan: CostPlan,
                                        store: CostStore, now: Date = Date(), calendar: Calendar = .current) -> [ProjectCost] {
        var fresh = range == .weekly ? store.currentPeriod(window: allowance) : store.rows(for: range)
        let nowTS = Int(now.timeIntervalSince1970)
        let from: Int
        if range == .weekly { from = store.currentPeriodStart(window: allowance) }
        else if let w = range.window { from = store.currentPeriodStart(window: w) }
        else if let d = range.start { from = Int(d.timeIntervalSince1970) }
        else { from = 0 }

        if plan.billing == .subscription, quotaBacked, range == .weekly {
            let perPoint = allowance == .credits ? plan.creditPointINR
                : plan.monthlyINR.map { $0 / CostEstimator.weeksPerMonth / 100 }
            for i in fresh.indices where !fresh[i].isUnexplained {
                let pct = fresh[i].pct
                fresh[i].cost = perPoint.map { CostValue.known($0 * pct) } ?? CostValue.unavailable
            }
            return fresh
        }

        let estimator = ActivityAccounting.estimator(plan: plan, store: store, from: from, to: nowTS, calendar: calendar)
        let costs = store.projectCosts(from: from, to: nowTS, estimator: estimator)
        for i in fresh.indices where !fresh[i].isUnexplained {
            fresh[i].cost = costs[fresh[i].project]?.estimatedSpend ?? .unavailable
        }
        return fresh
    }

    var store_: CostStore? { store }

    /// Diagnostics line for the settings menu.
    func statsLine() -> String? {
        guard let store else { return nil }
        let st = store.stats()
        guard st.events > 0 || st.files > 0 else { return nil }
        if st.errors > 0 { return L10n.t("\(st.events) turns · \(st.files) files · \(st.errors) skipped") }
        return L10n.t("\(st.events) turns · \(st.files) files")
    }
}


/// One CostModel per account (Claude and Codex), created lazily and kept while the account exists.
@MainActor
enum CostModels {
    private static var byAccount: [String: CostModel] = [:]
    /// Fires whenever any model's rows or expansion change (panel re-measures).
    static let anyChange = PassthroughSubject<Void, Never>()
    private static var subs: [String: [AnyCancellable]] = [:]

    static var all: [CostModel] {
        CostAccountStore.shared.accounts.compactMap { model(for: $0.id) }
    }

    static func model(for accountID: String) -> CostModel? {
        if let m = byAccount[accountID] { return m }
        guard let a = CostAccountStore.shared.account(accountID) else { return nil }
        let m = CostModel(account: a)
        byAccount[accountID] = m
        subs[accountID] = [
            m.$rows.dropFirst().sink { _ in anyChange.send() },
            m.$isExpanded.dropFirst().sink { _ in anyChange.send() },
        ]
        return m
    }
}

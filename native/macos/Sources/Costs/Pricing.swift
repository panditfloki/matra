import Foundation
import Combine

/// USD per million tokens for a model family (matched by prefix, longest wins).
///
/// `input` and `output` are required: a list entry without them is not a price.
/// The cache rates are optional because not every source publishes them, and a
/// rate nobody published must not read as free (see `Pricer.local`).
struct ModelPrice: Codable, Identifiable, Equatable, Sendable {
    var id: String { model }
    var model: String
    var input: Double
    var output: Double
    var cacheRead: Double?
    var cacheWrite: Double?

    /// Reads one list entry; nil when it does not carry a usable price.
    static func decode(_ d: [String: Any]) -> ModelPrice? {
        func rate(_ key: String) -> Double? {
            guard let value = d[key] as? Double, value.isFinite, value >= 0 else { return nil }
            return value
        }
        guard let model = d["model"] as? String, !model.isEmpty,
              let input = rate("input"), let output = rate("output") else { return nil }
        return ModelPrice(model: model, input: input, output: output,
                          cacheRead: rate("cacheRead"), cacheWrite: rate("cacheWrite"))
    }
}

/// Editable price list and exchange rate. Costs are estimates: the CLIs bill
/// through a subscription, this is what the same tokens would cost on the API.
@MainActor
final class PriceTable: ObservableObject {
    static let shared = PriceTable()

    @Published var prices: [ModelPrice] { didSet { save() } }
    @Published var rate: Double { didSet { save() } }          // 1 USD = `rate` INR; 0 = not known yet
    var rateKnown: Bool { rate > 0 }
    @Published var pricesUpdatedAt: Date? { didSet { save() } }
    @Published var rateUpdatedAt: Date? { didSet { save() } }
    @Published var lastError: String?

    static var fileURL: URL { CostRuntime.directory.appendingPathComponent("prices.json") }

    /// Fallback prices from the bundle (Resources/prices-default.json); replaced
    /// by OpenRouter's list on the first daily refresh.
    static let defaults: [ModelPrice] = {
        guard let url = Bundle.main.url(forResource: "prices-default", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let list = root["prices"] as? [[String: Any]] else { return [] }
        return list.compactMap(ModelPrice.decode)
    }()

    /// What a prices.json holds, after refusing anything that cannot be trusted.
    struct StoredState: Equatable {
        var prices: [ModelPrice] = []
        var rate: Double = 0
        var pricesUpdatedAt: Date?
        var rateUpdatedAt: Date?
    }

    /// A cached rate only counts when the file says it is a rate into INR. The
    /// cache used to hold a bare number, converted into whatever the Mac's
    /// currency was at the time; shown as rupees that would be a wrong amount
    /// under the right symbol, so an unlabelled or foreign rate reads as unknown.
    nonisolated static func decodeStored(_ data: Data) -> StoredState? {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        var state = StoredState()
        if let list = root["prices"] as? [[String: Any]] { state.prices = list.compactMap(ModelPrice.decode) }
        if let t = root["pricesUpdatedAt"] as? Double { state.pricesUpdatedAt = Date(timeIntervalSince1970: t) }
        if root["rateCurrency"] as? String == CostCurrency.code,
           let r = root["rate"] as? Double, r.isFinite, r > 0 {
            state.rate = r
            if let t = root["rateUpdatedAt"] as? Double { state.rateUpdatedAt = Date(timeIntervalSince1970: t) }
        }
        return state
    }

    private init() {
        prices = Self.defaults
        rate = 0                                   // unknown until fetched
        guard !CostRuntime.isInert else { return }
        if let data = try? Data(contentsOf: Self.fileURL), let state = Self.decodeStored(data) {
            if !state.prices.isEmpty { prices = state.prices }
            rate = state.rate
            pricesUpdatedAt = state.pricesUpdatedAt
            rateUpdatedAt = state.rateUpdatedAt
        }
        refreshIfDue()
        Timer.scheduledTimer(withTimeInterval: 6 * 3600, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.refreshIfDue() }
        }
    }

    // MARK: Daily refresh from public sources (no keys)

    static let modelsURL = URL(string: "https://openrouter.ai/api/v1/models")!
    static let fxURL = URL(string: "https://open.er-api.com/v6/latest/USD")!

    func refreshIfDue(force: Bool = false) {
        guard !CostRuntime.isInert else { return }
        let day: TimeInterval = 24 * 3600
        let needPrices = force || (pricesUpdatedAt.map { Date().timeIntervalSince($0) > day } ?? true)
        let needRate = force || !rateKnown || (rateUpdatedAt.map { Date().timeIntervalSince($0) > day } ?? true)
        guard needPrices || needRate else { return }
        Task.detached(priority: .utility) {
            var newPrices: [ModelPrice]?
            var newRate: Double?
            var failure: String?
            if needPrices {
                do {
                    let (data, response) = try await URLSession.shared.data(from: Self.modelsURL)
                    if (response as? HTTPURLResponse)?.statusCode == 200 { newPrices = Self.parseOpenRouter(data) }
                    else { failure = "OpenRouter: unexpected response" }
                } catch { failure = "OpenRouter: \(error.localizedDescription)" }
            }
            if needRate {
                do {
                    let (data, response) = try await URLSession.shared.data(from: Self.fxURL)
                    if (response as? HTTPURLResponse)?.statusCode == 200, let r = Self.parseFX(data) { newRate = r }
                    else { failure = "FX: unexpected response" }
                } catch { failure = "FX: \(error.localizedDescription)" }
            }
            let p = newPrices, r = newRate, e = failure
            await MainActor.run {
                if let p, !p.isEmpty { self.merge(p); self.pricesUpdatedAt = Date() }
                if let r { self.rate = r; self.rateUpdatedAt = Date() }
                self.lastError = e
            }
        }
    }

    /// USD to INR from the exchange-rate feed; nil unless the reply really is a
    /// successful USD-based table carrying a positive INR rate.
    nonisolated static func parseFX(_ data: Data) -> Double? {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              root["result"] as? String == "success",
              root["base_code"] as? String == "USD",
              let rates = root["rates"] as? [String: Any],
              let r = rates[CostCurrency.code] as? Double, r.isFinite, r > 0 else { return nil }
        return r
    }

    /// OpenRouter lists prices per token; keep the vendor models we care about
    /// as USD per million, keyed the way the CLIs name them. An entry without a
    /// real prompt and completion price is dropped, and a missing cache rate
    /// stays missing rather than reading as free.
    nonisolated static func parseOpenRouter(_ data: Data) -> [ModelPrice] {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let list = root["data"] as? [[String: Any]] else { return [] }
        var out: [ModelPrice] = []
        for m in list {
            guard let id = m["id"] as? String, !id.contains(":"),
                  id.hasPrefix("anthropic/") || id.hasPrefix("openai/"),
                  let pr = m["pricing"] as? [String: Any] else { continue }
            func rate(_ k: String) -> Double? {
                guard let raw = pr[k] as? String, let v = Double(raw), v.isFinite, v >= 0 else { return nil }
                return v * 1e6
            }
            guard let input = rate("prompt"), let output = rate("completion") else { continue }
            // "anthropic/claude-fable-5.1" → "claude-fable-5-1" (Claude Code's spelling)
            let name = id.split(separator: "/").last.map(String.init)!.replacingOccurrences(of: ".", with: "-")
            out.append(ModelPrice(model: name, input: input, output: output,
                                  cacheRead: rate("input_cache_read"), cacheWrite: rate("input_cache_write")))
        }
        return out
    }

    private func merge(_ fetched: [ModelPrice]) {
        var byModel = Dictionary(prices.map { ($0.model, $0) }, uniquingKeysWith: { _, new in new })
        for p in fetched where p.input > 0 || p.output > 0 { byModel[p.model] = p }
        prices = byModel.values.sorted { $0.model < $1.model }
    }

    private func save() {
        guard !CostRuntime.isInert else { return }
        var root: [String: Any] = [
            "rate": rate,
            "rateCurrency": CostCurrency.code,
            "prices": prices.map { price -> [String: Any] in
                var entry: [String: Any] = ["model": price.model, "input": price.input, "output": price.output]
                if let v = price.cacheRead { entry["cacheRead"] = v }
                if let v = price.cacheWrite { entry["cacheWrite"] = v }
                return entry
            },
        ]
        if let t = pricesUpdatedAt { root["pricesUpdatedAt"] = t.timeIntervalSince1970 }
        if let t = rateUpdatedAt { root["rateUpdatedAt"] = t.timeIntervalSince1970 }
        if let data = try? JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .sortedKeys]) {
            try? FileManager.default.createDirectory(at: Self.fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? data.write(to: Self.fileURL, options: .atomic)
        }
    }

    /// A snapshot usable off the main thread.
    var pricer: Pricer { Pricer(prices: prices, rate: rate) }
}

/// Prices one turn. Returns nil, never 0, when the answer is not known.
struct Pricer: Sendable {
    let prices: [ModelPrice]
    let rate: Double                  // 1 USD = `rate` INR; 0 = unknown

    /// Lower case, no vendor path, dots as dashes: "openai/gpt-5.1-codex" and
    /// OpenRouter's "gpt-5-1-codex" have to meet.
    private static func normalized(_ model: String) -> String {
        let bare = model.split(separator: "/").last.map(String.init) ?? model
        return bare.lowercased().replacingOccurrences(of: ".", with: "-")
    }

    /// A price belongs to this exact model. A newer version or a larger
    /// context variant may have different rates and must stay unpriced.
    func price(for model: String) -> ModelPrice? {
        let name = Self.normalized(model)
        return prices.first { Self.normalized($0.model) == name }
    }

    /// Rupees for one turn's tokens at API list price. Zero tokens cost nothing
    /// whatever the model; tokens with no rate, or no exchange rate, are unknown.
    func local(model: String, input: Int, output: Int, cacheRead: Int, cacheWrite: Int) -> Double? {
        guard input != 0 || output != 0 || cacheRead != 0 || cacheWrite != 0 else { return 0 }
        guard let p = price(for: model), rate > 0 else { return nil }
        var usd = Double(input) * p.input + Double(output) * p.output
        if cacheRead != 0 {
            guard let r = p.cacheRead else { return nil }
            usd += Double(cacheRead) * r
        }
        if cacheWrite != 0 {
            guard let r = p.cacheWrite else { return nil }
            usd += Double(cacheWrite) * r
        }
        return usd / 1_000_000 * rate
    }
}

enum MoneyFormat {
    /// Rupees with the Indian digit grouping, whatever the app's language:
    /// "₹1,23,457", "₹3.50", "₹0". A real amount too small to show is never
    /// rounded down into looking free.
    static func inr(_ amount: Double) -> String {
        if amount == 0 { return "₹0" }
        if amount > 0, amount < 0.01 { return "< ₹0.01" }
        let f = NumberFormatter()
        f.locale = Locale(identifier: "en_IN")
        f.numberStyle = .currency
        f.currencyCode = CostCurrency.code
        f.currencySymbol = "₹"
        let digits = abs(amount) < 10 ? 2 : 0
        f.minimumFractionDigits = digits
        f.maximumFractionDigits = digits
        return f.string(from: NSNumber(value: amount)) ?? String(format: "₹%.2f", amount)
    }
    static func tokens(_ n: Int) -> String {
        if n >= 1_000_000 { return String(format: "%.1fM", Double(n) / 1_000_000) }
        if n >= 1_000 { return String(format: "%.0fk", Double(n) / 1_000) }
        return "\(n)"
    }
}


/// Turns one turn's tokens into money for one account, off the main thread.
///   subscription: the plan's cost for the limit period holding the turn, split
///                 by token weight; with no limit periods at all, the price of
///                 the calendar month holding the turn over that month's weight.
///   api:          the price table, converted at the USD rate.
/// A turn the data does not cover is unknown (nil), never the nearest period.
struct CostEstimator: Sendable {
    static let weeksPerMonth = 365.25 / 12 / 7   // 4.35

    let billing: CostAccount.Billing
    /// INR per month; nil when neither the owner nor the catalog says.
    let monthlyPrice: Double?
    /// Credit-based seat: money per 1 % of the allowance (limit × price ÷ 100).
    var creditPointValue: Double? = nil
    /// The seat is credit-based even if `creditPointValue` is not known yet, so
    /// its usage is unknown rather than priced as a monthly plan.
    var creditBacked = false
    /// Limit periods with, for each, how much of the allowance was used and the
    /// total token weight of the account's turns inside it. Half open, sorted,
    /// non-overlapping (see `CostStore.allocationPeriods`).
    let periods: [(start: Int, end: Int, usedPct: Double, weight: Double)]
    /// Token weight of each calendar month, keyed by `monthStart(of:)`.
    var monthWeights: [Int: Double] = [:]
    var calendar: Calendar = .current
    let pricer: Pricer

    /// The account's own cost for this turn: its plan share, or on an API key
    /// what the tokens cost at list price.
    func cost(at ts: Int, weight: Double, model: String, input: Int, output: Int, cacheRead: Int, cacheWrite: Int) -> Double? {
        switch billing {
        case .subscription:
            return planShare(at: ts, weight: weight)
        case .api:
            return pricer.local(model: model, input: input, output: output, cacheRead: cacheRead, cacheWrite: cacheWrite)
        }
    }

    /// This turn's slice of the plan price. Always an estimate, never a bill.
    func planShare(at ts: Int, weight: Double) -> Double? {
        guard weight > 0 else { return 0 }           // nothing to allocate
        let period = periods.first { ts >= $0.start && ts < $0.end }
        if creditBacked || creditPointValue != nil {
            guard let pointValue = creditPointValue, let p = period, p.weight > 0 else { return nil }
            return pointValue * p.usedPct * weight / p.weight
        }
        guard let monthlyPrice else { return nil }
        if !periods.isEmpty {
            // The account has limit periods and this turn is in none of them:
            // how much of the allowance it used was never measured.
            guard let p = period, p.weight > 0 else { return nil }
            let weekSpend = monthlyPrice / Self.weeksPerMonth * p.usedPct / 100
            return weekSpend * weight / p.weight
        }
        // No limit periods at all: spread that month's price over that month's tokens.
        guard let month = monthWeights[Self.monthStart(of: ts, calendar: calendar)], month > 0 else { return nil }
        return monthlyPrice * weight / month
    }

    static func weight(input: Int, output: Int, cacheRead: Int, cacheWrite: Int) -> Double {
        Double(input) + Double(output) * CostStore.kOutput
            + Double(cacheRead) * CostStore.kCacheRead + Double(cacheWrite) * CostStore.kCacheWrite
    }

    static func monthStart(of ts: Int, calendar: Calendar = .current) -> Int {
        let date = Date(timeIntervalSince1970: TimeInterval(ts))
        let start = calendar.dateInterval(of: .month, for: date)?.start ?? date
        return Int(start.timeIntervalSince1970)
    }
}


/// Subscription plans and their list prices, from a JSON catalog the user can
/// edit (`costs/plans.json` in the app's support folder, seeded from the bundle)
/// or point at a URL that is re-read once a day. Nothing about plans lives in code.
@MainActor
final class PlanCatalog: ObservableObject {
    static let shared = PlanCatalog()

    struct Plan: Sendable { var name: String; var prices: [String: Double]; var creditUSD: Double? }
    @Published private(set) var plans: [String: Plan] = [:]
    @Published private(set) var updatedAt: Date?

    static var fileURL: URL { CostRuntime.directory.appendingPathComponent("plans.json") }
    static let remoteKey = "plansURL"
    var remoteURL: String {
        get { UserDefaults.standard.string(forKey: Self.remoteKey) ?? "" }
        set {
            guard !CostRuntime.isInert else { return }
            UserDefaults.standard.set(newValue, forKey: Self.remoteKey); objectWillChange.send(); refreshIfDue(force: true)
        }
    }

    private init() {
        guard !CostRuntime.isInert else { return }
        if !FileManager.default.fileExists(atPath: Self.fileURL.path),
           let bundled = Bundle.main.url(forResource: "plans", withExtension: "json") {
            try? FileManager.default.createDirectory(at: Self.fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? FileManager.default.copyItem(at: bundled, to: Self.fileURL)
        }
        load()
        refreshIfDue()
        Timer.scheduledTimer(withTimeInterval: 6 * 3600, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.refreshIfDue() }
        }
    }

    private func load() {
        guard let data = try? Data(contentsOf: Self.fileURL) else { return }
        apply(data)
        updatedAt = (try? FileManager.default.attributesOfItem(atPath: Self.fileURL.path))?[.modificationDate] as? Date
    }

    private func apply(_ data: Data) {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let list = root["plans"] as? [String: [String: Any]] else { return }
        var out: [String: Plan] = [:]
        for (tier, d) in list {
            let prices = (d["prices"] as? [String: Any])?.compactMapValues { ($0 as? Double) ?? ($0 as? Int).map(Double.init) } ?? [:]
            out[tier] = Plan(name: d["name"] as? String ?? tier, prices: prices,
                             creditUSD: (d["credit_usd"] as? Double) ?? (d["credit_usd"] as? Int).map(Double.init))
        }
        plans = out
    }

    /// Re-read a remote catalog at most once a day, when one is configured.
    func refreshIfDue(force: Bool = false) {
        guard !CostRuntime.isInert else { return }
        guard let url = URL(string: remoteURL), !remoteURL.isEmpty else { return }
        if !force, let at = updatedAt, Date().timeIntervalSince(at) < 24 * 3600 { return }
        Task.detached(priority: .utility) {
            guard let (data, resp) = try? await URLSession.shared.data(from: url),
                  (resp as? HTTPURLResponse)?.statusCode == 200 else { return }
            await MainActor.run {
                self.apply(data)
                try? data.write(to: Self.fileURL, options: .atomic)
                self.updatedAt = Date()
            }
        }
    }

    func name(for tier: String) -> String {
        plans[tier]?.name ?? tier.replacingOccurrences(of: "default_claude_", with: "").replacingOccurrences(of: "_", with: " ").capitalized
    }

    /// Monthly list price in INR: the catalog's rupee price when it has one, else
    /// its USD price at `rate`. Nil when neither is possible; a free plan is a
    /// real ₹0, not a missing price.
    nonisolated static func monthlyINR(plan: Plan?, rate: Double) -> Double? {
        guard let plan else { return nil }
        if let local = plan.prices[CostCurrency.code] { return local }
        guard let usd = plan.prices["USD"] else { return nil }
        if usd == 0 { return 0 }
        return rate > 0 ? usd * rate : nil
    }

    func monthlyINR(for tier: String, rate: Double) -> Double? { Self.monthlyINR(plan: plans[tier], rate: rate) }
    func usd(for tier: String) -> Double? { plans[tier]?.prices["USD"] }
    func creditUSD(for tier: String) -> Double? { plans[tier]?.creditUSD }
}

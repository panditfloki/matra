import Foundation
import CoreFoundation

/// Numeric summaries only. Local-device activity is deliberately not assigned
/// to a signed-in account, nor combined with account-wide profile token totals.
struct UsageSpendReport: Equatable, Sendable {
    struct Value: Equatable, Sendable {
        var tokens: Int
        var costUSD: Double?
        var incompleteCost: Bool = false
        /// Tokens proven to carry an offline price. Nil when the report
        /// cannot tell; never guessed from a cost total.
        var pricedTokens: Int? = nil

        /// Explicit coverage, else only what the cost fields alone prove.
        var knownPricedTokens: Int? {
            if let pricedTokens { return min(pricedTokens, tokens) }
            if tokens == 0 || costUSD == nil { return 0 }
            return incompleteCost ? nil : tokens
        }

        static func sum(_ values: [Value]) -> Value {
            let known = values.compactMap(\.costUSD)
            let priced = values.map(\.knownPricedTokens)
            return Value(tokens: values.reduce(0) { $0 + $1.tokens },
                         costUSD: known.isEmpty ? nil : known.reduce(0, +),
                         incompleteCost: values.contains { $0.incompleteCost || $0.costUSD == nil },
                         pricedTokens: priced.contains(nil) ? nil : priced.reduce(0) { $0 + ($1 ?? 0) })
        }
    }
    /// One report row's models (with tokens) and the cost ccusage gave it.
    struct PricingEvidence: Equatable, Sendable {
        let models: Set<String>
        let costUSD: Double
    }
    struct Breakdown: Identifiable, Equatable, Sendable {
        let id: String
        let value: Value
    }
    struct Day: Identifiable, Equatable, Sendable {
        let id: String
        let value: Value
        let models: [Breakdown]
    }
    let providerID: String
    let measuredAt: Date
    let timeZoneID: String
    let days: [Day]
    let projects: [Breakdown]
    let pricingNote: String
    var staleReason: String?

    var total: Value { Value.sum(days(in: .month, now: measuredAt).map(\.value)) }
    var models: [Breakdown] {
        Self.group(days.flatMap(\.models))
    }

    func days(in period: SpendPeriod, now: Date) -> [Day] {
        let calendar = calendar
        let today = calendar.startOfDay(for: now)
        let start: Date
        switch period {
        case .today: start = today
        case .week: start = calendar.date(byAdding: .day, value: -6, to: today) ?? today
        case .month: start = calendar.date(byAdding: .day, value: -29, to: today) ?? today
        case .monthToDate: start = calendar.dateInterval(of: .month, for: now)?.start ?? today
        }
        let lower = Self.dayKey(start, calendar: calendar)
        let upper = Self.dayKey(today, calendar: calendar)
        return days.filter { $0.id >= lower && $0.id <= upper }
    }

    func today(now: Date) -> Value? {
        days.first { $0.id == Self.dayKey(now, calendar: calendar) }?.value
    }

    /// Never put a whole day's activity inside a five-hour window. Only fully
    /// contained, completed calendar days can be a lower-bound window subtotal.
    func completeDaySubtotal(start: Date, end: Date, now: Date) -> Value? {
        let included = days.filter { day in
            guard let date = date(for: day.id),
                  let next = calendar.date(byAdding: .day, value: 1, to: date) else { return false }
            return date >= start && next <= min(end, now)
        }
        return included.isEmpty ? nil : Value.sum(included.map(\.value))
    }

    var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: timeZoneID) ?? .current
        return calendar
    }
    func date(for key: String) -> Date? {
        let components = key.split(separator: "-").compactMap { Int($0) }
        guard components.count == 3 else { return nil }
        return calendar.date(from: DateComponents(year: components[0], month: components[1], day: components[2]))
    }
    static func dayKey(_ date: Date, calendar: Calendar) -> String {
        let c = calendar.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", c.year ?? 0, c.month ?? 0, c.day ?? 0)
    }
    static func group(_ rows: [Breakdown]) -> [Breakdown] {
        Dictionary(grouping: rows, by: \.id).map { key, values in
            Breakdown(id: key, value: Value.sum(values.map(\.value)))
        }.sorted { $0.value.tokens > $1.value.tokens }
    }

    /// Adapter for the installed ccusage CLI. Reasoning is already included in
    /// totalTokens; cache tokens are not added to this canonical total again.
    /// `evidence` adds rows from other reports (Codex sessions) that help
    /// prove which models carry an offline price.
    static func decodeDaily(_ data: Data, providerID: String, measuredAt: Date,
                            timeZoneID: String, evidence extra: [PricingEvidence] = []) throws -> UsageSpendReport {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rawDays = object["daily"] as? [[String: Any]] else { throw SpendReaderError.invalidReport }
        var hasFallback = false
        var evidence = extra
        typealias Row = (key: String, tokens: Int, cost: Double?, models: [Breakdown], itemised: Bool)
        let rows = try rawDays.map { row -> Row in
            guard let key = row["date"] as? String, validDay(key),
                  let tokens = integer(row["totalTokens"]) else { throw SpendReaderError.invalidReport }
            var models: [Breakdown] = []
            var itemised = false
            if let map = row["models"] as? [String: [String: Any]] {
                // Codex rows carry no per-model cost. Treat them as itemised
                // only if every model really has its own cost field.
                itemised = !map.isEmpty && map.values.allSatisfy { money($0["costUSD"]) != nil }
                models = map.sorted { $0.key < $1.key }.map { name, metric in
                    if metric["isFallback"] as? Bool == true { hasFallback = true }
                    let n = integer(metric["totalTokens"]) ?? 0
                    let cost = money(metric["costUSD"])
                    return Breakdown(id: name, value: Value(tokens: n, costUSD: n > 0 && cost == 0 ? nil : cost))
                }
            } else if let list = row["modelBreakdowns"] as? [[String: Any]] {
                itemised = true
                models = list.compactMap { metric in
                    guard let name = metric["modelName"] as? String else { return nil }
                    let n = ["inputTokens", "outputTokens", "cacheReadTokens", "cacheCreationTokens"]
                        .reduce(0) { $0 + (integer(metric[$1]) ?? 0) }
                    let cost = money(metric["cost"])
                    // Offline CLI reports unknown model prices as zero. This
                    // is an estimate source, not proof of free token usage.
                    return Breakdown(id: name, value: Value(tokens: n,
                        costUSD: n > 0 && cost == 0 ? nil : cost))
                }
            }
            let cost = money(row["costUSD"] ?? row["totalCost"])
            let used = Set(models.filter { $0.value.tokens > 0 }.map(\.id))
            if !itemised, !used.isEmpty, let cost { evidence.append(PricingEvidence(models: used, costUSD: cost)) }
            return (key, tokens, cost, models, itemised)
        }
        let pricing = modelPricing(evidence)
        let days = rows.map { row -> Day in
            let models = row.models.map { model -> Breakdown in
                var value = model.value
                let priced = row.itemised ? value.costUSD != nil : pricing[model.id] == true
                value.pricedTokens = priced || value.tokens == 0 ? value.tokens : 0
                return Breakdown(id: model.id, value: value)
            }
            // Without model rows coverage cannot be proven either way.
            let priced = models.isEmpty ? nil : min(row.tokens, models.reduce(0) { $0 + ($1.value.pricedTokens ?? 0) })
            let partial = priced.map { $0 < row.tokens } ?? false
            var cost = row.cost
            if row.itemised && partial { cost = Value.sum(models.map(\.value)).costUSD }
            if row.tokens > 0 && cost == 0 { cost = nil }
            return Day(id: row.key, value: Value(tokens: row.tokens, costUSD: cost,
                                                 incompleteCost: partial, pricedTokens: priced), models: models)
        }.sorted { $0.id < $1.id }
        guard Set(days.map(\.id)).count == days.count else { throw SpendReaderError.invalidReport }
        let unpriced = group(days.flatMap(\.models)).filter { $0.value.tokens > 0 && $0.value.pricedTokens == 0 }.map(\.id).sorted()
        var note = hasFallback ? "Offline API-price estimate. Some models use fallback pricing. Not your subscription bill."
            : "Offline API-price estimate from ccusage. Missing prices stay unavailable. Not your subscription bill."
        if !unpriced.isEmpty {
            note += " No offline price for \(unpriced.joined(separator: ", ")): their tokens are counted, their cost is not."
        }
        return UsageSpendReport(providerID: providerID, measuredAt: measuredAt,
            timeZoneID: timeZoneID, days: days, projects: [], pricingNote: note)
    }

    /// ccusage's offline table prices unknown models at zero, and Codex rows
    /// carry no per-model cost. A row costing zero proves all its models
    /// unpriced; a priced row whose other models are all unpriced proves the
    /// remaining one priced. Anything else stays unknown and is never counted
    /// as priced, so its cost can only ever be a lower bound.
    static func modelPricing(_ evidence: [PricingEvidence]) -> [String: Bool] {
        var status: [String: Bool] = [:]
        var changed = true
        while changed {
            changed = false
            for row in evidence where !row.models.isEmpty {
                if row.costUSD == 0 {
                    for model in row.models where status[model] == nil { status[model] = false; changed = true }
                } else {
                    let open = row.models.filter { status[$0] != false }
                    if open.count == 1, let model = open.first, status[model] == nil { status[model] = true; changed = true }
                }
            }
        }
        return status
    }

    /// Pricing evidence from a Codex session report. Sessions often run on a
    /// single model, which proves that model's price status.
    static func sessionEvidence(_ data: Data) -> [PricingEvidence] {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let sessions = root["sessions"] as? [[String: Any]] else { return [] }
        return sessions.compactMap { row in
            guard let map = row["models"] as? [String: [String: Any]], let cost = money(row["costUSD"]) else { return nil }
            let used = Set(map.filter { (integer($0.value["totalTokens"]) ?? 0) > 0 }.keys)
            return used.isEmpty ? nil : PricingEvidence(models: used, costUSD: cost)
        }
    }

    static func integer(_ value: Any?) -> Int? {
        guard let n = value as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID(),
              n.doubleValue.isFinite, n.doubleValue >= 0, n.doubleValue < Double(Int.max),
              n.doubleValue.rounded() == n.doubleValue else { return nil }
        return n.intValue
    }
    static func money(_ value: Any?) -> Double? {
        guard let n = value as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID(),
              n.doubleValue.isFinite, n.doubleValue >= 0 else { return nil }
        return n.doubleValue
    }
    private static func validDay(_ key: String) -> Bool {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        f.isLenient = false
        guard let date = f.date(from: key) else { return false }
        return f.string(from: date) == key
    }
}

enum SpendPeriod: String, CaseIterable, Identifiable {
    case today = "Today", week = "7 days", month = "30 days", monthToDate = "This month"
    var id: String { rawValue }
}

enum SpendReaderError: LocalizedError {
    case unavailable, invalidReport, failed, timedOut
    var errorDescription: String? {
        switch self {
        case .unavailable: return "Local analytics needs an existing ccusage installation."
        case .invalidReport: return "The local report format could not be read."
        case .failed: return "Local analytics could not be refreshed."
        case .timedOut: return "Local analytics took too long to refresh."
        }
    }
}

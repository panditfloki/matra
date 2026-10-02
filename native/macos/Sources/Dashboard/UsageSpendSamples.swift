import Foundation

enum UsageSpendSamples {
    static func reports(now: Date = Date()) -> [String: UsageSpendReport] {
        let calendar = Calendar.current
        return Dictionary(uniqueKeysWithValues: ["codex", "claude", "gemini"].enumerated().map { index, id in
            let names = id == "codex" ? ["gpt-6.1-sol", "gpt-6-astra"]
                : id == "claude" ? ["claude-opus-5-5", "claude-sonnet-5-5"] : ["gemini-3.8-flash"]
            let days = (0..<30).map { offset -> UsageSpendReport.Day in
                let date = calendar.date(byAdding: .day, value: offset - 29, to: now)!
                let n = (offset % 7 + 1) * 420_000 * (index + 1)
                let models = names.enumerated().map { i, name in
                    UsageSpendReport.Breakdown(id: name,
                        value: .init(tokens: names.count == 1 ? n : (i == 0 ? n * 3 / 4 : n / 4),
                                     costUSD: Double(n) / 1_000_000 * (i == 0 ? 3.2 : 1.4)))
                }
                return .init(id: UsageSpendReport.dayKey(date, calendar: calendar), value: .sum(models.map(\.value)), models: models)
            }
            let total = UsageSpendReport.Value.sum(days.map(\.value))
            let projects: [UsageSpendReport.Breakdown] = id == "gemini" ? [] : [
                .init(id: "Mātrā · Sample project", value: .init(tokens: total.tokens * 3 / 4, costUSD: (total.costUSD ?? 0) * 0.75)),
                .init(id: "Demo workspace · Sample project", value: .init(tokens: total.tokens / 4, costUSD: (total.costUSD ?? 0) * 0.25))]
            return (id, UsageSpendReport(providerID: id, measuredAt: now, timeZoneID: calendar.timeZone.identifier,
                days: days, projects: projects, pricingNote: "Sample API-price estimate, not account usage or a subscription bill."))
        })
    }
}

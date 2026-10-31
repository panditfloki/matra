import XCTest
@testable import Matra

/// The money the cost layer reports: each turn priced on its own, nothing
/// unknown reading as ₹0, and every total the sum of the same pieces. All data
/// is fixture data in temporary stores; no real profile or transcript is read.
final class MatraCostAccountingTests: XCTestCase {
    private var directory: URL!
    private var calendar: Calendar!

    private let opus = ModelPrice(model: "claude-opus-5", input: 10, output: 50, cacheRead: 1, cacheWrite: 12)
    private let sonnet = ModelPrice(model: "claude-sonnet-5", input: 1, output: 5, cacheRead: 0.1, cacheWrite: 1.2)
    private let free = ModelPrice(model: "free-model", input: 0, output: 0, cacheRead: 0, cacheWrite: 0)
    private var pricer: Pricer { Pricer(prices: [opus, sonnet, free], rate: 100) }

    override func setUpWithError() throws {
        directory = MatraTestFiles.temporaryDirectory
            .appendingPathComponent("MatraCostAccountingTests-\(UUID().uuidString)", isDirectory: true)
        calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try XCTUnwrap(TimeZone(identifier: "Asia/Kolkata"))
    }

    override func tearDownWithError() throws {
        if let directory { try? FileManager.default.removeItem(at: directory) }
    }

    // MARK: Fixtures

    private func date(_ y: Int, _ mo: Int, _ d: Int, _ h: Int = 12, _ mi: Int = 0) -> Date {
        calendar.date(from: DateComponents(year: y, month: mo, day: d, hour: h, minute: mi))!
    }
    private func ts(_ y: Int, _ mo: Int, _ d: Int, _ h: Int = 12, _ mi: Int = 0) -> Int {
        Int(date(y, mo, d, h, mi).timeIntervalSince1970)
    }

    private func makeStore(_ name: String = UUID().uuidString) throws -> CostStore {
        try XCTUnwrap(CostStore(url: directory.appendingPathComponent("\(name).sqlite")))
    }

    private func insert(_ store: CostStore, ts: Int, session: String = "s1", project: String = "/p/one",
                        model: String = "claude-opus-5", input: Int = 1_000_000, output: Int = 0,
                        cacheRead: Int = 0, key: String = UUID().uuidString) {
        let event = UsageEvent(ts: ts, sessionId: session, dedupeKey: key, project: project, cwd: project,
                               branch: nil, model: model, input: input, output: output,
                               cacheRead: cacheRead, cacheWrite: 0, ccVersion: nil)
        store.commit(events: [event], path: "/fixture/\(UUID().uuidString)", inode: 1, size: 1, offset: 1, mtime: 0)
    }

    private func source(_ store: CostStore, id: String = "a", plan: CostPlan) -> ActivitySource {
        ActivitySource(accountID: id, accountName: id.uppercased(), accountIndex: 0, plan: plan, store: store)
    }
    private func api() -> CostPlan { CostPlan(billing: .api, monthlyINR: nil, pricer: pricer) }
    private func subscription(_ monthly: Double?) -> CostPlan {
        CostPlan(billing: .subscription, monthlyINR: monthly, pricer: pricer)
    }

    private func report(_ sources: [ActivitySource], _ interval: DateInterval) -> ActivityReport {
        ActivityAccounting.report(sources: sources, interval: interval, calendar: calendar)
    }
    private func dayInterval(_ y: Int, _ mo: Int, _ d: Int) -> DateInterval {
        ActivityPeriod.day.interval(containing: date(y, mo, d), calendar: calendar)
    }
    private func monthInterval(_ y: Int, _ mo: Int) -> DateInterval {
        ActivityPeriod.month.interval(containing: date(y, mo, 10), calendar: calendar)
    }

    // MARK: Per-turn pricing

    /// Opus then Sonnet in one session: the old aggregate priced everything at
    /// the last model, here 1M × ₹1,000 rather than 2M × ₹100.
    func testMixedModelSessionPricesEachTurnAtItsOwnModel() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 10, 10), model: "claude-opus-5")
        insert(store, ts: ts(2026, 3, 10, 11), model: "claude-sonnet-5")

        let r = report([source(store, plan: api())], dayInterval(2026, 3, 10))
        let session = try XCTUnwrap(r.sessions.first)
        XCTAssertEqual(r.sessions.count, 1)
        // 1M input: Opus $10 → ₹1,000, Sonnet $1 → ₹100.
        XCTAssertEqual(session.totals.apiKeyUsage.amount, 1_100, accuracy: 0.001)
        XCTAssertEqual(session.totals.apiKeyUsage.coverage, .complete)
        XCTAssertEqual(session.models, ["claude-opus-5", "claude-sonnet-5"])
    }

    func testPartlyPricedSessionReadsAtLeast() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 10, 10), model: "claude-opus-5")
        insert(store, ts: ts(2026, 3, 10, 11), model: "mystery-1", input: 500)

        let total = report([source(store, plan: api())], dayInterval(2026, 3, 10)).total.apiKeyUsage
        XCTAssertEqual(total.coverage, .atLeast)
        XCTAssertEqual(total.amount, 1_000, accuracy: 0.001)
        XCTAssertEqual(total.unknownTurns, 1)
        XCTAssertEqual(total.unknownTokens, 500)
        XCTAssertEqual(total.unknownModels, ["mystery-1"])
        XCTAssertEqual(total.text, "At least ₹1,000")
    }

    func testNothingPricedIsUnavailableNotZero() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 10), model: "mystery-1")

        let total = report([source(store, plan: api())], dayInterval(2026, 3, 10)).total.estimatedSpend
        XCTAssertEqual(total.coverage, .unavailable)
        XCTAssertEqual(total.text, "Unavailable")
    }

    func testValidZeroCostDiffersFromUnknown() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 10), model: "free-model")
        let zero = report([source(store, plan: api())], dayInterval(2026, 3, 10)).total.apiKeyUsage
        XCTAssertEqual(zero.coverage, .complete)
        XCTAssertEqual(zero.amount, 0)
        XCTAssertEqual(zero.text, "₹0")

        // No turns at all is neither of those.
        XCTAssertEqual(CostValue().coverage, .none)
        XCTAssertEqual(CostValue().text, "-")
        // Zero tokens have nothing to price, even on a model with no price.
        XCTAssertEqual(pricer.local(model: "mystery-1", input: 0, output: 0, cacheRead: 0, cacheWrite: 0), 0)
        XCTAssertNil(pricer.local(model: "mystery-1", input: 1, output: 0, cacheRead: 0, cacheWrite: 0))
    }

    /// Without an exchange rate a dollar list price has no rupee value, while an
    /// amount the owner typed in rupees still does.
    func testUnknownExchangeRateOnlyAffectsListPrices() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 10))
        let noRate = Pricer(prices: [opus], rate: 0)
        let plan = CostPlan(billing: .subscription, monthlyINR: 8_000, pricer: noRate)

        let total = report([source(store, plan: plan)], dayInterval(2026, 3, 10)).total
        XCTAssertEqual(total.apiEquivalent.coverage, .unavailable)
        XCTAssertEqual(total.planShare.coverage, .complete)
        XCTAssertEqual(total.planShare.amount, 8_000, accuracy: 0.001)
    }

    func testMissingCacheRateIsUnknownNotFree() {
        let partial = Pricer(prices: [ModelPrice(model: "gpt-x", input: 1, output: 1, cacheRead: nil, cacheWrite: nil)], rate: 100)
        XCTAssertNil(partial.local(model: "gpt-x", input: 10, output: 10, cacheRead: 10, cacheWrite: 0))
        XCTAssertNotNil(partial.local(model: "gpt-x", input: 10, output: 10, cacheRead: 0, cacheWrite: 0))
    }

    func testModelMatchingRespectsFamilyBoundaries() {
        let p = Pricer(prices: [opus, ModelPrice(model: "gpt-5", input: 1, output: 1),
                                ModelPrice(model: "gpt-5-1-codex", input: 2, output: 2)], rate: 1)
        XCTAssertNil(p.price(for: "claude-opus-5-5[1m]"))
        XCTAssertNil(p.price(for: "claude-opus-55"))
        XCTAssertEqual(p.price(for: "openai/gpt-5.1-codex")?.model, "gpt-5-1-codex")
        XCTAssertNil(p.price(for: "gpt-5.6-sol"))
    }

    // MARK: Interval, account and project isolation

    func testIntervalExcludesOutsideTurnsAndSplitsAcrossMidnight() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 9, 23, 50), session: "night")
        insert(store, ts: ts(2026, 3, 10, 0, 10), session: "night")
        insert(store, ts: ts(2026, 3, 11, 9), session: "later")

        let day = report([source(store, plan: api())], dayInterval(2026, 3, 10))
        XCTAssertEqual(day.total.turns, 1)
        XCTAssertEqual(day.sessions.map(\.sessionID), ["night"])

        let week = report([source(store, plan: api())], DateInterval(start: date(2026, 3, 9, 0), end: date(2026, 3, 12, 0)))
        XCTAssertEqual(week.total.turns, 3)
        // The midnight session is on both days, not all on the day it began.
        XCTAssertEqual(week.days.map(\.totals.turns), [1, 1, 1])
        XCTAssertEqual(week.sessions.first { $0.sessionID == "night" }?.totals.turns, 2)
    }

    func testAccountsAndProjectsAreIsolatedAndReconcile() throws {
        let a = try makeStore("a"), b = try makeStore("b")
        insert(a, ts: ts(2026, 3, 10, 9), session: "s1", project: "/p/one")
        insert(a, ts: ts(2026, 3, 10, 10), session: "s1", project: "/p/two", model: "claude-sonnet-5")
        insert(a, ts: ts(2026, 3, 10, 11), session: "s2", project: "/p/two", model: "mystery-1")
        insert(b, ts: ts(2026, 3, 10, 9), session: "s1", project: "/p/one", model: "claude-sonnet-5")

        let r = report([source(a, id: "a", plan: api()), source(b, id: "b", plan: api())], dayInterval(2026, 3, 10))
        let onlyA = r.scoped(accountID: "a"), onlyB = r.scoped(accountID: "b")
        XCTAssertEqual(onlyA.total.apiKeyUsage.amount, 1_100, accuracy: 0.001)
        XCTAssertEqual(onlyB.total.apiKeyUsage.amount, 100, accuracy: 0.001)
        XCTAssertEqual(onlyB.total.apiKeyUsage.coverage, .complete)
        XCTAssertEqual(onlyA.total.apiKeyUsage.coverage, .atLeast)
        XCTAssertEqual(r.scoped(project: "/p/one").total.apiKeyUsage.amount, 1_100, accuracy: 0.001)
        XCTAssertEqual(r.scoped(accountID: "a", project: "/p/two").total.apiKeyUsage.coverage, .atLeast)
        // One session id in two accounts stays two rows; one session in two projects too.
        XCTAssertEqual(r.sessions.count, 4)

        assertReconciles(r)
        assertReconciles(onlyA)
        assertSame(onlyA.total + onlyB.total, r.total)
    }

    private func assertReconciles(_ r: ActivityReport, file: StaticString = #filePath, line: UInt = #line) {
        assertSame(r.sessions.reduce(CostTotals()) { $0 + $1.totals }, r.total, file: file, line: line)
        assertSame(r.projects.reduce(CostTotals()) { $0 + $1.totals }, r.total, file: file, line: line)
        assertSame(r.accounts.reduce(CostTotals()) { $0 + $1.totals }, r.total, file: file, line: line)
        assertSame(r.days.reduce(CostTotals()) { $0 + $1.totals }, r.total, file: file, line: line)
        XCTAssertEqual(r.projects.reduce(0) { $0 + $1.duration }, r.duration, accuracy: 0.001, file: file, line: line)
        XCTAssertEqual(r.days.reduce(0) { $0 + $1.duration }, r.duration, accuracy: 0.001, file: file, line: line)
        let spend = r.total.planShare.amount + r.total.apiKeyUsage.amount
        XCTAssertEqual(r.total.estimatedSpend.amount, spend, accuracy: 0.001, file: file, line: line)
    }

    private func assertSame(_ a: CostTotals, _ b: CostTotals, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(a.tokens, b.tokens, file: file, line: line)
        XCTAssertEqual(a.turns, b.turns, file: file, line: line)
        for (x, y) in [(a.planShare, b.planShare), (a.apiKeyUsage, b.apiKeyUsage), (a.apiEquivalent, b.apiEquivalent)] {
            XCTAssertEqual(x.amount, y.amount, accuracy: 0.001, file: file, line: line)
            XCTAssertEqual(x.pricedTurns, y.pricedTurns, file: file, line: line)
            XCTAssertEqual(x.unknownTurns, y.unknownTurns, file: file, line: line)
            XCTAssertEqual(x.unknownTokens, y.unknownTokens, file: file, line: line)
            XCTAssertEqual(x.unknownModels, y.unknownModels, file: file, line: line)
        }
    }

    // MARK: Subscription attribution

    /// A turn outside every measured limit period is unknown. It used to be
    /// charged against the most recent period, however long ago the turn was.
    func testTurnOutsideLimitPeriodsIsUnknownNotTheLastPeriod() throws {
        let store = try makeStore()
        let resets = date(2026, 3, 20)
        store.recordSample(window: .weekly, pct: 50, resetsAt: resets, at: date(2026, 3, 15))
        insert(store, ts: ts(2026, 3, 16), session: "inside")
        insert(store, ts: ts(2026, 2, 1), session: "before")

        let r = report([source(store, plan: subscription(4_350))], monthInterval(2026, 3))
        // A week = ₹1,000; 50% used = ₹500; the only turn in the period takes all of it.
        XCTAssertEqual(r.total.planShare.amount, 500, accuracy: 0.5)

        let february = report([source(store, plan: subscription(4_350))], monthInterval(2026, 2))
        XCTAssertEqual(february.total.planShare.coverage, .unavailable)
        XCTAssertEqual(february.total.planShare.text, "Unavailable")
        // The list-price comparison does not depend on the limit, and stays known.
        XCTAssertEqual(february.total.apiEquivalent.coverage, .complete)
    }

    func testPeriodBoundaryBelongsToTheNextPeriod() {
        let periods: [(start: Int, end: Int, usedPct: Double, weight: Double)] = [(0, 100, 10, 10), (100, 200, 20, 10)]
        let e = CostEstimator(billing: .subscription, monthlyPrice: CostEstimator.weeksPerMonth * 100, periods: periods, pricer: pricer)
        XCTAssertEqual(e.planShare(at: 99, weight: 10) ?? -1, 10, accuracy: 0.001)
        XCTAssertEqual(e.planShare(at: 100, weight: 10) ?? -1, 20, accuracy: 0.001)
        XCTAssertNil(e.planShare(at: 200, weight: 10))
    }

    /// With no limit periods a past date is divided by its own month's tokens.
    /// The old denominator was always the current month, whichever date was open.
    func testHistoricalMonthUsesItsOwnDenominator() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 1, 10), session: "jan", input: 1_000)
        insert(store, ts: ts(2026, 1, 20), session: "jan2", input: 3_000)
        insert(store, ts: ts(2026, 3, 5), session: "mar", input: 9_000_000)

        let january = report([source(store, plan: subscription(1_000))], monthInterval(2026, 1))
        XCTAssertEqual(january.total.planShare.amount, 1_000, accuracy: 0.001)
        XCTAssertEqual(january.sessions.first { $0.sessionID == "jan" }?.totals.planShare.amount ?? -1, 250, accuracy: 0.001)

        // One day of January is still a share of all of January, not of that day.
        let tenth = report([source(store, plan: subscription(1_000))], dayInterval(2026, 1, 10))
        XCTAssertEqual(tenth.total.planShare.amount, 250, accuracy: 0.001)
    }

    func testUnknownPlanPriceIsUnavailableAndFreePlanIsZero() throws {
        let store = try makeStore()
        insert(store, ts: ts(2026, 3, 10))
        XCTAssertEqual(report([source(store, plan: subscription(nil))], dayInterval(2026, 3, 10)).total.planShare.coverage, .unavailable)
        let free = report([source(store, plan: subscription(0))], dayInterval(2026, 3, 10)).total.planShare
        XCTAssertEqual(free.coverage, .complete)
        XCTAssertEqual(free.text, "₹0")
    }

    func testCreditSeatWithoutACreditPriceIsUnavailable() throws {
        let store = try makeStore()
        store.recordSample(window: .credits, pct: 40, resetsAt: date(2026, 3, 30), at: date(2026, 3, 1))
        insert(store, ts: ts(2026, 3, 10))
        // A monthly price must not stand in for a credit price nobody knows.
        let unpriced = CostPlan(billing: .subscription, monthlyINR: 5_000, creditPointINR: nil, pricer: pricer)
        XCTAssertEqual(report([source(store, plan: unpriced)], dayInterval(2026, 3, 10)).total.planShare.coverage, .unavailable)

        let priced = CostPlan(billing: .subscription, monthlyINR: nil, creditPointINR: 25, pricer: pricer)
        // 40 points × ₹25, all of it on the only turn.
        XCTAssertEqual(report([source(store, plan: priced)], dayInterval(2026, 3, 10)).total.planShare.amount, 1_000, accuracy: 0.001)
    }

    /// Use the transcripts cannot explain is the card's "Elsewhere", not a cost
    /// of these local turns.
    func testElsewhereUseIsNotChargedToLocalTurns() throws {
        let store = try makeStore()
        let resets = date(2026, 3, 20)
        store.recordSample(window: .weekly, pct: 0, resetsAt: resets, at: date(2026, 3, 14))
        store.recordSample(window: .weekly, pct: 30, resetsAt: resets, at: date(2026, 3, 15))   // no turns: elsewhere
        insert(store, ts: ts(2026, 3, 16))
        store.recordSample(window: .weekly, pct: 50, resetsAt: resets, at: date(2026, 3, 17))

        let plan = subscription(4_350)
        let r = report([source(store, plan: plan)], monthInterval(2026, 3))
        // A week = ₹1,000; 50% used, of which 30% elsewhere: ₹200 here.
        XCTAssertEqual(r.total.planShare.amount, 200, accuracy: 0.5)
    }

    func testWobblingResetTimeIsOnePeriod() throws {
        let store = try makeStore()
        let resets = ts(2026, 3, 20)
        store.recordSample(window: .weekly, pct: 10, resetsAt: Date(timeIntervalSince1970: TimeInterval(resets)), at: date(2026, 3, 15))
        store.recordSample(window: .weekly, pct: 20, resetsAt: Date(timeIntervalSince1970: TimeInterval(resets + 60)), at: date(2026, 3, 16))
        store.recordSample(window: .weekly, pct: 5, resetsAt: Date(timeIntervalSince1970: TimeInterval(resets + 7 * 86_400)), at: date(2026, 3, 21))

        let periods = store.periods(window: .weekly)
        XCTAssertEqual(periods.count, 2)
        XCTAssertEqual(periods[0].usedPct, 20)
        XCTAssertEqual(periods[0].end, resets + 60)
        XCTAssertLessThanOrEqual(periods[0].end, periods[1].start)
    }

    // MARK: The hover card

    func testCardRowsKeepUnknownVisible() throws {
        let store = try makeStore()
        let now = date(2026, 3, 18)
        insert(store, ts: ts(2026, 3, 18, 9), project: "/p/known", model: "claude-opus-5")
        insert(store, ts: ts(2026, 3, 18, 10), project: "/p/mystery", model: "mystery-1")
        let rows = CostModel.computeRows(range: .allTime, allowance: .weekly, quotaBacked: false, plan: api(),
                                         store: store, now: now, calendar: calendar)
        XCTAssertEqual(rows.first { $0.project == "/p/known" }?.cost?.coverage, .complete)
        XCTAssertEqual(rows.first { $0.project == "/p/mystery" }?.cost?.coverage, .unavailable)
    }

    func testAllowanceRowsHaveNoAmountWithoutAPlanPrice() throws {
        let store = try makeStore()
        let now = Date()
        let resets = now.addingTimeInterval(3 * 86_400)
        store.recordSample(window: .weekly, pct: 10, resetsAt: resets, at: now.addingTimeInterval(-3_600))
        insert(store, ts: Int(now.timeIntervalSince1970) - 1_800)
        store.recordSample(window: .weekly, pct: 20, resetsAt: resets, at: now.addingTimeInterval(-60))

        let rows = CostModel.computeRows(range: .weekly, allowance: .weekly, quotaBacked: true, plan: subscription(nil),
                                         store: store, now: now, calendar: calendar)
        XCTAssertFalse(rows.isEmpty)
        for row in rows where !row.isUnexplained { XCTAssertEqual(row.cost?.coverage, .unavailable) }

        let priced = CostModel.computeRows(range: .weekly, allowance: .weekly, quotaBacked: true, plan: subscription(4_350),
                                           store: store, now: now, calendar: calendar)
        let total = priced.compactMap(\.cost).reduce(CostValue(), +)
        // A week = ₹1,000, 20% used: the rows together carry ₹200.
        XCTAssertEqual(total.amount, 200, accuracy: 0.5)
    }

    // MARK: INR

    func testAmountsAreRupeesWhateverTheLocale() {
        XCTAssertEqual(MoneyFormat.inr(123_456.7), "₹1,23,457")
        XCTAssertEqual(MoneyFormat.inr(3.5), "₹3.50")
        XCTAssertEqual(MoneyFormat.inr(0), "₹0")
        XCTAssertEqual(MoneyFormat.inr(0.001), "< ₹0.01")
        XCTAssertEqual(CostCurrency.code, "INR")
        XCTAssertEqual(CostValue.known(1_500).text, "₹1,500")
    }

    func testCachedRateMustBeLabelledINR() throws {
        func stored(_ json: String) throws -> PriceTable.StoredState {
            try XCTUnwrap(PriceTable.decodeStored(Data(json.utf8)))
        }
        // A bare number from before the currency was fixed could be anything.
        XCTAssertEqual(try stored(#"{"rate": 5.4, "rateUpdatedAt": 1000}"#).rate, 0)
        XCTAssertNil(try stored(#"{"rate": 5.4, "rateUpdatedAt": 1000}"#).rateUpdatedAt)
        XCTAssertEqual(try stored(#"{"rate": 5.4, "rateCurrency": "BRL"}"#).rate, 0)
        XCTAssertEqual(try stored(#"{"rate": -3, "rateCurrency": "INR"}"#).rate, 0)
        XCTAssertEqual(try stored(#"{"rate": 84.5, "rateCurrency": "INR"}"#).rate, 84.5)
    }

    func testFXReplyMustBeAUSDTableWithINR() {
        func fx(_ json: String) -> Double? { PriceTable.parseFX(Data(json.utf8)) }
        XCTAssertEqual(fx(#"{"result":"success","base_code":"USD","rates":{"INR":84.5,"BRL":5.4}}"#), 84.5)
        XCTAssertNil(fx(#"{"result":"success","base_code":"USD","rates":{"BRL":5.4}}"#))
        XCTAssertNil(fx(#"{"result":"success","base_code":"EUR","rates":{"INR":90}}"#))
        XCTAssertNil(fx(#"{"result":"error","base_code":"USD","rates":{"INR":84.5}}"#))
        XCTAssertNil(fx(#"{"result":"success","base_code":"USD","rates":{"INR":0}}"#))
    }

    func testOpenRouterParsingDoesNotInventPrices() throws {
        let json = #"""
        {"data":[
          {"id":"anthropic/claude-x-1.5","pricing":{"prompt":"0.000003","completion":"0.000015","input_cache_read":"0.0000003"}},
          {"id":"openai/router","pricing":{"prompt":"-1","completion":"-1"}},
          {"id":"openai/no-completion","pricing":{"prompt":"0.000001"}}
        ]}
        """#
        let parsed = PriceTable.parseOpenRouter(Data(json.utf8))
        XCTAssertEqual(parsed.map(\.model), ["claude-x-1-5"])
        let price = try XCTUnwrap(parsed.first)
        XCTAssertEqual(price.input, 3, accuracy: 0.0001)
        XCTAssertEqual(price.cacheRead ?? -1, 0.3, accuracy: 0.0001)
        XCTAssertNil(price.cacheWrite)
    }

    func testPlanPriceComesFromRupeesOrAKnownRate() {
        let dollars = PlanCatalog.Plan(name: "Max", prices: ["USD": 100, "BRL": 550], creditUSD: nil)
        XCTAssertEqual(PlanCatalog.monthlyINR(plan: dollars, rate: 84), 8_400)
        XCTAssertNil(PlanCatalog.monthlyINR(plan: dollars, rate: 0))
        let rupees = PlanCatalog.Plan(name: "Max", prices: ["USD": 100, "INR": 9_000], creditUSD: nil)
        XCTAssertEqual(PlanCatalog.monthlyINR(plan: rupees, rate: 84), 9_000)
        let free = PlanCatalog.Plan(name: "Free", prices: ["USD": 0], creditUSD: nil)
        XCTAssertEqual(PlanCatalog.monthlyINR(plan: free, rate: 0), 0)
        XCTAssertNil(PlanCatalog.monthlyINR(plan: nil, rate: 84))
    }

    @MainActor func testManualPriceIsRupeesPaidAndBeatsTheCatalog() {
        var account = CostAccount(id: "x", provider: "claude", name: "X", configDirectory: URL(fileURLWithPath: "/nonexistent"))
        XCTAssertNil(account.monthlyINR(rate: 84))
        account.monthlyPaidINR = 8_500
        XCTAssertEqual(account.monthlyINR(rate: 0), 8_500)
        account.monthlyPaidINR = 0
        XCTAssertEqual(account.monthlyINR(rate: 84), 0)
    }

    func testOlderManualPriceInAnUnknownCurrencyIsNotReadAsRupees() throws {
        let data = Data(#"{"claude":{"billing":"subscription","monthlyPrice":550}}"#.utf8)
        let map = try JSONDecoder().decode([String: CostAccountStore.Stored].self, from: data)
        XCTAssertNil(map["claude"]?.monthlyPaidINR)
    }

    // MARK: Latest request wins

    @MainActor func testSlowEarlierLoadCannotReplaceANewerReport() async throws {
        let slow = DispatchSemaphore(value: 0)
        let finished = expectation(description: "the slow load ran to its end")
        let old = ActivityReport(slices: [slice(session: "old")])
        let new = ActivityReport(slices: [slice(session: "new")])
        let loader = ActivityReportLoader()

        loader.load {
            slow.wait()
            finished.fulfill()
            return old
        }
        loader.load { new }
        while loader.loading { try await Task.sleep(nanoseconds: 5_000_000) }
        XCTAssertEqual(loader.report, new)

        slow.signal()
        await fulfillment(of: [finished], timeout: 5)
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(loader.report, new)
        XCTAssertEqual(loader.scoped.sessions.map(\.sessionID), ["new"])
        XCTAssertFalse(loader.loading)
    }

    @MainActor func testLoaderScopesWithoutReloading() async throws {
        let loader = ActivityReportLoader()
        let both = ActivityReport(slices: [slice(session: "s1", account: "a", project: "/p/one"),
                                           slice(session: "s2", account: "b", project: "/p/two")])
        loader.projectFilter = "/p/two"
        loader.load { both }
        while loader.loading { try await Task.sleep(nanoseconds: 5_000_000) }
        XCTAssertEqual(loader.scoped.sessions.map(\.sessionID), ["s2"])
        loader.projectFilter = ""
        loader.accountFilter = "a"
        XCTAssertEqual(loader.scoped.sessions.map(\.sessionID), ["s1"])

        // A project the new range does not contain cannot stay selected.
        loader.accountFilter = ""
        loader.projectFilter = "/p/two"
        let other = ActivityReport(slices: [slice(session: "s3", account: "a", project: "/p/one")])
        loader.load { other }
        while loader.loading { try await Task.sleep(nanoseconds: 5_000_000) }
        XCTAssertEqual(loader.projectFilter, "")
        XCTAssertEqual(loader.scoped.sessions.map(\.sessionID), ["s3"])
    }

    private func slice(session: String, account: String = "a", project: String = "/p/one") -> ActivitySlice {
        ActivitySlice(accountID: account, accountName: account, accountIndex: 0, billing: .api, sessionID: session,
                      project: project, cwd: project, branch: nil, day: Date(timeIntervalSince1970: 0),
                      first: 0, last: 0, models: [])
    }

    // MARK: Containment

    @MainActor func testSingletonsAreInertInATestRun() {
        XCTAssertTrue(CostRuntime.isInert)
        XCTAssertTrue(CostAccountStore.shared.accounts.isEmpty)
        CostAccountStore.shared.rediscover()
        XCTAssertTrue(CostAccountStore.shared.accounts.isEmpty)
        XCTAssertFalse(PriceTable.shared.rateKnown)
        XCTAssertTrue(PlanCatalog.shared.plans.isEmpty)
        XCTAssertNil(CostModels.model(for: "claude"))

        let account = CostAccount(id: "test-x", provider: "claude", name: "Test", configDirectory: URL(fileURLWithPath: "/nonexistent"))
        XCTAssertNil(CostModel(account: account).store_)
    }

    func testOnlyASampleOrLocalLogPreviewIsInert() {
        XCTAssertFalse(CostRuntime.isInert(underTest: false, designPreview: false, livePreview: false))
        XCTAssertTrue(CostRuntime.isInert(underTest: true, designPreview: false, livePreview: false))
        XCTAssertTrue(CostRuntime.isInert(underTest: false, designPreview: true, livePreview: false))
        XCTAssertFalse(CostRuntime.isInert(underTest: false, designPreview: true, livePreview: true))
    }

    @MainActor func testEveryCostFileLivesUnderTheAppsOwnSupportRoot() {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let root = MatraStorage.supportRoot(in: base).appendingPathComponent("costs", isDirectory: true).standardizedFileURL.path
        XCTAssertEqual(CostRuntime.directory.standardizedFileURL.path, root)
        for url in [PriceTable.fileURL, PlanCatalog.fileURL, CostAccountStore.fileURL] {
            XCTAssertEqual(url.deletingLastPathComponent().standardizedFileURL.path, root)
        }
    }

    /// The Activity window names sessions from metadata. It must never go back
    /// to opening a transcript for the first prompt.
    func testActivityTitlesComeFromMetadataOnly() throws {
        var s = slice(session: "0123456789abcdef")
        XCTAssertEqual(ActivityReport(slices: [s]).sessions.first?.title, "01234567")
        s.branch = "feature/x"
        XCTAssertEqual(ActivityReport(slices: [s]).sessions.first?.title, "feature/x · 01234567")

        let source = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appendingPathComponent("../Sources/Costs/ActivityWindow.swift").standardizedFileURL
        let text = try String(contentsOf: source, encoding: .utf8)
        XCTAssertFalse(text.contains("TranscriptTitles"))
        XCTAssertFalse(text.contains(".jsonl"))
        XCTAssertFalse(text.contains("FileHandle"))
    }
}

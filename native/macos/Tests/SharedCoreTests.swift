import Foundation
import XCTest
@testable import Matra

/// Exercises the real Mac model adapters against the same vectors as Rust. No profiles or services.
final class SharedCoreTests: XCTestCase {
    private func rows(_ file: String) throws -> [[String]] {
        let native = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let url = native.appendingPathComponent("matra-core/tests/fixtures/\(file).tsv")
        return try String(contentsOf: url, encoding: .utf8).split(separator: "\n")
            .filter { !$0.hasPrefix("#") }.map { $0.split(separator: "\t", omittingEmptySubsequences: false).map(String.init) }
    }
    private func number(_ text: String) -> Double? { text == "-" || text == "_" ? nil : Double(text) }

    func testProductionPricerUsesSharedVectors() throws {
        XCTAssertEqual(SharedCore.abiVersion, 1)
        for r in try rows("pricing") {
            let price = ModelPrice(model: r[1], input: try XCTUnwrap(number(r[7])), output: try XCTUnwrap(number(r[8])),
                                   cacheRead: number(r[9]), cacheWrite: number(r[10]))
            let pricer = Pricer(prices: [price], rate: try XCTUnwrap(number(r[11])))
            let input = Int(r[3])!, output = Int(r[4])!, read = Int(r[5])!, write = Int(r[6])!
            XCTAssertEqual(pricer.price(for: r[2]) != nil, r[14] == "1", r[0])
            let actual = pricer.local(model: r[2], input: input, output: output, cacheRead: read, cacheWrite: write)
            if let expected = number(r[12]) {
                XCTAssertEqual(try XCTUnwrap(actual, r[0]), expected, accuracy: 1e-10 * max(abs(expected), 1), r[0])
            } else { XCTAssertNil(actual, r[0]) }
            XCTAssertEqual(CostEstimator.weight(input: input, output: output, cacheRead: read, cacheWrite: write),
                           try XCTUnwrap(number(r[13])), accuracy: 1e-9, r[0])
        }
    }

    func testProductionCostValueUsesSharedCoverage() throws {
        let coverage: [CostValue.Coverage] = [.none, .complete, .atLeast, .unavailable]
        for r in try rows("coverage") {
            var value = CostValue()
            if r[1] != "-" { for part in r[1].split(separator: ",") { value.add(number(String(part))) } }
            let expected = try XCTUnwrap(number(r[2]))
            XCTAssertEqual(value.amount, expected, accuracy: 1e-10 * max(abs(expected), 1), r[0])
            XCTAssertEqual(value.pricedTurns, Int(r[3]), r[0])
            XCTAssertEqual(value.unknownTurns, Int(r[4]), r[0])
            XCTAssertEqual(value.coverage, coverage[Int(r[5])!], r[0])
        }
        XCTAssertEqual((CostValue.known(0) + .unavailable).coverage, .atLeast)
    }

    func testProductionClaudeOrderingAndHeadlineUseSharedVectors() throws {
        for r in try rows("quota") {
            let entries = r[1] == "-" ? [] : r[1].split(separator: "|").map { $0.split(separator: ":").map(String.init) }
            let windows = entries.map { entry in
                LimitWindow(id: entry[0], label: entry[0], usedFraction: 0.5,
                            money: entry[1] == "1" ? UsageMoneyBreakdown(currency: "USD", spent: 1, remaining: 1) : nil)
            }
            XCTAssertEqual(UsageResponse.headlineID(for: windows), r[2] == "-" ? "session" : r[2], r[0])
            let order = windows.sorted(by: UsageResponse.displayOrder).map(\.id)
            XCTAssertEqual(order.isEmpty ? "-" : order.joined(separator: "|"), r[3], r[0])
        }
    }
}

import Foundation

@main
struct SharedCoreContract {
    static func rows(_ path: String) throws -> [[String]] {
        try String(contentsOfFile: path, encoding: .utf8).split(separator: "\n")
            .filter { !$0.hasPrefix("#") }.map { $0.split(separator: "\t", omittingEmptySubsequences: false).map(String.init) }
    }
    static func number(_ text: String) -> Double? { text == "-" || text == "_" ? nil : Double(text)! }
    static func check(_ condition: @autoclosure () -> Bool, _ name: String) {
        precondition(condition(), name)
    }
    static func close(_ actual: Double, _ expected: Double, _ name: String) {
        check(abs(actual - expected) <= 1e-10 * max(abs(expected), 1), name)
    }
    static func main() throws {
        let fixtures = CommandLine.arguments[1]
        check(SharedCore.abiVersion == 1, "C ABI version")
        var vectors = 0
        for r in try rows(fixtures + "/pricing.tsv") {
            let input = Int(r[3])!, output = Int(r[4])!, read = Int(r[5])!, write = Int(r[6])!
            let matches = SharedCore.modelsMatch(r[1], r[2])
            check(matches == (r[14] == "1"), r[0] + " model identity")
            let result = SharedCore.localCost(input: input, output: output, cacheRead: read, cacheWrite: write,
                inputRate: matches ? number(r[7]) : nil, outputRate: matches ? number(r[8]) : nil,
                cacheReadRate: number(r[9]), cacheWriteRate: number(r[10]), usdToINR: number(r[11])!)
            let pricer = Pricer(prices: [ModelPrice(model: r[1], input: number(r[7])!, output: number(r[8])!,
                cacheRead: number(r[9]), cacheWrite: number(r[10]))], rate: number(r[11])!)
            let production = pricer.local(model: r[2], input: input, output: output, cacheRead: read, cacheWrite: write)
            check(production == result, r[0] + " real native Mac Pricer")
            check(CostEstimator.weight(input: input, output: output, cacheRead: read, cacheWrite: write)
                == SharedCore.weight(input: input, output: output, cacheRead: read, cacheWrite: write), r[0] + " real CostEstimator")
            if let expected = number(r[12]) {
                check(result != nil, r[0] + " must be known")
                close(result!, expected, r[0])
            } else { check(result == nil, r[0] + " must stay unknown") }
            close(SharedCore.weight(input: input, output: output, cacheRead: read, cacheWrite: write), number(r[13])!, r[0] + " weight")
            vectors += 1
        }
        for r in try rows(fixtures + "/coverage.tsv") {
            var sum = (amount: 0.0, priced: 0, unknown: 0)
            if r[1] != "-" {
                for text in r[1].split(separator: ",") {
                    sum = SharedCore.accumulate(amount: sum.amount, priced: sum.priced, unknown: sum.unknown, cost: number(String(text)))
                }
            }
            close(sum.amount, number(r[2])!, r[0])
            check(sum.priced == Int(r[3])!, r[0] + " priced turns")
            check(sum.unknown == Int(r[4])!, r[0] + " unknown turns")
            check(SharedCore.coverage(priced: sum.priced, unknown: sum.unknown) == UInt32(r[5])!, r[0] + " coverage")
            vectors += 1
        }
        for r in try rows(fixtures + "/quota.tsv") {
            let windows = r[1] == "-" ? [] : r[1].split(separator: "|").map { $0.split(separator: ":").map(String.init) }
            let head = windows.map { (id: $0[0], rank: SharedCore.claudeHeadlineRank(id: $0[0], isBalance: $0[1] == "1")) }
                .filter { $0.rank != UInt32.max }.min { $0.rank < $1.rank }?.id ?? "-"
            check(head == r[2], r[0] + " headline")
            let order = windows.map { $0[0] }.sorted {
                let left = SharedCore.claudeDisplayRank(id: $0), right = SharedCore.claudeDisplayRank(id: $1)
                return left == right ? $0 < $1 : left < right
            }
            check((order.isEmpty ? "-" : order.joined(separator: "|")) == r[3], r[0] + " display order")
            vectors += 1
        }
        var differential = 0
        for input in [0, 1, 10_001, 1_000_000] {
            for output in [0, 1, 203, 500_000] {
                for cached in [0, 1, 71_117] {
                    // Frozen original Mac arithmetic, not an invocation of the shared implementation.
                    var usd = Double(input) * 3 + Double(output) * 15
                    if cached != 0 { usd += Double(cached) * 0.3 }
                    if cached / 2 != 0 { usd += Double(cached / 2) * 3.75 }
                    let original = usd / 1_000_000 * 83.4
                    let result = SharedCore.localCost(input: input, output: output, cacheRead: cached, cacheWrite: cached / 2,
                        inputRate: 3, outputRate: 15, cacheReadRate: 0.3, cacheWriteRate: 3.75, usdToINR: 83.4)
                    check(result == original, "native Mac reference differential")
                    let originalWeight = Double(input) + Double(output) * 5 + Double(cached) * 0.1 + Double(cached / 2) * 1.25
                    check(SharedCore.weight(input: input, output: output, cacheRead: cached, cacheWrite: cached / 2) == originalWeight,
                          "native Mac reference weight differential")
                    differential += 1
                }
            }
        }
        check(SharedCore.localCost(input: -1, output: 0, cacheRead: 0, cacheWrite: 0,
            inputRate: 1, outputRate: 1, cacheReadRate: nil, cacheWriteRate: nil, usdToINR: 100) == nil,
            "negative token inputs stay unknown")
        print("Swift production Pricer → Rust C ABI: \(vectors) shared vectors and \(differential) native Mac differential cases passed")
    }
}

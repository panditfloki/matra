import Foundation

/// Only value conversion lives here. The linked Rust core owns the shared business rules.
/// No UI, account discovery, preferences, calendar or storage passes through this boundary.
enum SharedCore {
    static var abiVersion: UInt32 { matra_core_abi_version() }
    static var outputWeight: Double { matra_core_weight_output() }
    static var cacheReadWeight: Double { matra_core_weight_cache_read() }
    static var cacheWriteWeight: Double { matra_core_weight_cache_write() }

    private static func utf8<T>(_ text: String, _ body: (UnsafePointer<UInt8>?, Int) -> T) -> T {
        Array(text.utf8).withUnsafeBufferPointer { body($0.baseAddress, $0.count) }
    }

    static func modelsMatch(_ left: String, _ right: String) -> Bool {
        utf8(left) { a, countA in utf8(right) { b, countB in
            matra_core_models_match(a, countA, b, countB) != 0
        } }
    }

    private static func tokens(input: Int, output: Int, cacheRead: Int, cacheWrite: Int) -> MatraCoreTokens? {
        guard input >= 0, output >= 0, cacheRead >= 0, cacheWrite >= 0 else { return nil }
        return MatraCoreTokens(input: UInt64(input), output: UInt64(output),
                               cache_read: UInt64(cacheRead), cache_write: UInt64(cacheWrite))
    }

    static func weight(input: Int, output: Int, cacheRead: Int, cacheWrite: Int) -> Double {
        guard let tokens = tokens(input: input, output: output, cacheRead: cacheRead, cacheWrite: cacheWrite)
        else { return 0 }
        return matra_core_token_weight(tokens)
    }

    static func localCost(input: Int, output: Int, cacheRead: Int, cacheWrite: Int,
                          inputRate: Double?, outputRate: Double?, cacheReadRate: Double?, cacheWriteRate: Double?,
                          usdToINR: Double) -> Double? {
        guard let tokens = tokens(input: input, output: output, cacheRead: cacheRead, cacheWrite: cacheWrite)
        else { return nil }
        let rates = MatraCorePriceRates(input: inputRate ?? 0, output: outputRate ?? 0,
                                        cache_read: cacheReadRate ?? 0, cache_write: cacheWriteRate ?? 0,
                                        has_cache_read: cacheReadRate == nil ? 0 : 1,
                                        has_cache_write: cacheWriteRate == nil ? 0 : 1)
        let result = matra_core_local_cost(tokens, rates, inputRate != nil && outputRate != nil ? 1 : 0, usdToINR)
        return result.known == 0 ? nil : result.value
    }

    static func accumulate(amount: Double, priced: Int, unknown: Int, cost: Double?) -> (amount: Double, priced: Int, unknown: Int) {
        let result = matra_core_cost_add(MatraCoreAccumulator(amount: amount, priced: UInt64(max(0, priced)), unknown: UInt64(max(0, unknown))),
                                         MatraCoreOptionalAmount(value: cost ?? 0, known: cost == nil ? 0 : 1))
        return (result.amount, Int(clamping: result.priced), Int(clamping: result.unknown))
    }

    static func merge(amount: Double, priced: Int, unknown: Int, otherAmount: Double, otherPriced: Int, otherUnknown: Int)
        -> (amount: Double, priced: Int, unknown: Int) {
        let result = matra_core_cost_merge(
            MatraCoreAccumulator(amount: amount, priced: UInt64(max(0, priced)), unknown: UInt64(max(0, unknown))),
            MatraCoreAccumulator(amount: otherAmount, priced: UInt64(max(0, otherPriced)), unknown: UInt64(max(0, otherUnknown))))
        return (result.amount, Int(clamping: result.priced), Int(clamping: result.unknown))
    }

    static func coverage(priced: Int, unknown: Int) -> UInt32 {
        matra_core_coverage(UInt64(max(0, priced)), UInt64(max(0, unknown)))
    }

    static func claudeHeadlineRank(id: String, isBalance: Bool) -> UInt32 {
        utf8(id) { matra_core_claude_headline_rank($0, $1, isBalance ? 1 : 0) }
    }
    static func claudeDisplayRank(id: String) -> UInt32 {
        utf8(id) { matra_core_claude_display_rank($0, $1) }
    }
}

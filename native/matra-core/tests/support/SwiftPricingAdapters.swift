#if MATRA_CORE_STANDALONE_TEST
import Foundation

// Only host services needed to compile the real Pricing.swift without starting an app.
// The contract runner uses Pricer and CostEstimator; these inert adapters forbid storage access.
enum CostRuntime {
    static let isInert = true
    static var directory: URL { preconditionFailure("Standalone core tests must not access cost storage") }
}
enum CostCurrency { static let code = "INR" }
enum CostAccount { enum Billing { case subscription, api } }
#endif

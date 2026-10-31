import Foundation

/// Known monthly credit allowances for Higgsfield subscription tiers.
enum HiggsfieldPlan {
    static let allowances: [String: Double] = [
        "ultra": 3000,
        "plus": 1200,
        "pro": 1200,
        "starter": 250,
        "basic": 150,
        "free": 0
    ]

    static func allowance(for planType: String) -> Double? {
        let normalized = planType.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return allowances[normalized]
    }
}

/// Parses Higgsfield's workspace listings or account credit responses.
///
/// Accepts:
/// 1. Array of workspace objects from `hf workspace list --json` or
///    `GET https://fnf-api-gw.higgsfield.ai/fnf/developer/v2alpha/account/workspaces`:
///    ```json
///    [
///      {
///        "id": "ws_123",
///        "name": "Personal",
///        "plan_type": "ultra",
///        "credits": 2777.1,
///        "is_selected": true,
///        "user_role": "owner"
///      }
///    ]
///    ```
/// 2. Dictionary wrapping workspaces: `{ "workspaces": [ ... ] }`
/// 3. Direct account or MCP balance object:
///    `{ "credits": 2777.1, "subscription_plan_type": "ultra" }`
enum HiggsfieldUsage {
    struct Result {
        let windows: [LimitWindow]
        let headlineID: String?
        let planName: String?
        let workspaceName: String?
        /// `.derived` when the percentage comes from this file's plan table
        /// rather than an allowance the account itself reported.
        let fidelity: Fidelity
    }

    static func parse(_ jsonString: String) throws -> Result {
        guard let data = jsonString.data(using: .utf8) else {
            throw UsageProviderError.badResponse(status: 0)
        }
        return try parse(data)
    }

    static func parse(_ data: Data) throws -> Result {
        guard let json = try? JSONSerialization.jsonObject(with: data) else {
            throw UsageProviderError.badResponse(status: 0)
        }

        let workspaces: [[String: Any]]
        if let array = json as? [[String: Any]] {
            workspaces = array
        } else if let dict = json as? [String: Any] {
            if let array = dict["workspaces"] as? [[String: Any]] {
                workspaces = array
            } else if dict["credits"] != nil || dict["plan_type"] != nil || dict["subscription_plan_type"] != nil {
                workspaces = [dict]
            } else {
                workspaces = []
            }
        } else {
            throw UsageProviderError.badResponse(status: 0)
        }

        guard !workspaces.isEmpty else {
            throw UsageProviderError.nothingMetered(
                L10n.t("No Higgsfield workspaces or credits found.")
            )
        }

        // The selected workspace, or the only one there is. Several with none
        // selected would mean showing some other workspace's balance as this
        // one's, so that is not a reading.
        guard let active = workspaces.first(where: { ($0["is_selected"] as? Bool) == true })
                ?? (workspaces.count == 1 ? workspaces[0] : nil) else {
            throw UsageProviderError.nothingMetered(
                L10n.t("No Higgsfield workspaces or credits found.")
            )
        }

        guard let creditsNumber = number(active["credits"]) else {
            throw UsageProviderError.nothingMetered(
                L10n.t("No Higgsfield credit balance reported.")
            )
        }

        let planType = (active["plan_type"] as? String)
            ?? (active["subscription_plan_type"] as? String)
            ?? (active["plan"] as? String)

        let workspaceName = active["name"] as? String

        let explicitAllowance = number(active["credit_limit"])
            ?? number(active["allowance"])
            ?? number(active["monthly_credits"])

        let allowance = explicitAllowance ?? planType.flatMap { HiggsfieldPlan.allowance(for: $0) }

        let resetDate = date(active["resets_at"] ?? active["reset_at"] ?? active["period_end"] ?? active["reset_date"])

        // An overdrawn balance has nothing left to spend, not a negative amount.
        let roundedCredits = Int(max(creditsNumber, 0).rounded())
        var windows: [LimitWindow] = []

        if let allowance, allowance > 0 {
            let roundedAllowance = Int(allowance.rounded())
            let usedCredits = max(0, allowance - creditsNumber)
            let usedFraction = min(max(usedCredits / allowance, 0), 1)
            let roundedUsed = max(0, roundedAllowance - roundedCredits)
            let extraCredits = max(0, creditsNumber - allowance)

            let detail: String
            if extraCredits > 0 {
                let roundedExtra = Int(extraCredits.rounded())
                detail = L10n.t("\(roundedCredits) of \(roundedAllowance) · +\(roundedExtra) extra")
            } else {
                detail = L10n.t("\(roundedCredits) of \(roundedAllowance) left")
            }

            windows.append(LimitWindow(
                id: "credits",
                label: L10n.t("Credits"),
                usedFraction: usedFraction,
                remaining: roundedCredits,
                used: roundedUsed,
                detail: detail,
                resetsAt: resetDate
            ))

            if extraCredits > 0 {
                let roundedExtra = Int(extraCredits.rounded())
                windows.append(LimitWindow(
                    id: "extra_credits",
                    label: L10n.t("Extra credits"),
                    used: roundedExtra,
                    detail: "\(roundedExtra)",
                    resetsAt: resetDate
                ))
            }
        } else {
            // Plan has no known monthly allowance or is pay-as-you-go: show balance without guessing fraction
            windows.append(LimitWindow(
                id: "credits",
                label: L10n.t("Credits"),
                remaining: roundedCredits,
                detail: L10n.t("\(roundedCredits) credits left"),
                resetsAt: resetDate
            ))
        }

        return Result(
            windows: windows,
            headlineID: "credits",
            planName: planType?.replacingOccurrences(of: "_", with: " ").capitalized,
            workspaceName: workspaceName,
            fidelity: explicitAllowance == nil && (allowance ?? 0) > 0 ? .derived : .official
        )
    }

    /// A count this file can do arithmetic on. `JSONSerialization` answers `true`
    /// with an `NSNumber` worth 1, and `Int(_:)` traps past its range, so neither
    /// a boolean nor an absurd magnitude gets through.
    private static func number(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID()
        else { return nil }
        let double = number.doubleValue
        return double.isFinite && abs(double) < 1e15 ? double : nil
    }

    private static func date(_ value: Any?) -> Date? {
        guard let text = value as? String else { return nil }
        let withFraction = ISO8601DateFormatter()
        withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let plain = ISO8601DateFormatter()
        plain.formatOptions = [.withInternetDateTime]
        return withFraction.date(from: text) ?? plain.date(from: text)
    }
}

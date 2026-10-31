import SwiftUI

/// Settings › Costs: how each login is paid, the plan detected for it, and
/// the market data the money is worked out from.
struct CostSettingsPane: View {
    @ObservedObject var accounts: CostAccountStore = .shared
    @ObservedObject var prices: PriceTable = .shared
    @ObservedObject var catalog: PlanCatalog = .shared

    var body: some View {
        Form {
            ForEach(accounts.accounts) { account in
                Section(account.name) {
                    CostAccountRows(account: account)
                }
            }

            Section(L10n.t("How the money is worked out")) {
                Text(L10n.t("All amounts are in Indian rupees (INR) and all are estimates, not bills. Plan share: a week of the plan costs the monthly amount ÷ 4.35, and a project that used 4% of the weekly allowance is allocated 4% of that. API-equivalent: the same tokens at API list prices, converted to INR. Where a price or a limit reading is missing the amount shows Unavailable or At least, never ₹0."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Section(L10n.t("Market data")) {
                LabeledContent(L10n.t("Exchange rate")) {
                    Text(prices.rateKnown ? L10n.t("1 USD = \(MoneyFormat.inr(prices.rate))") : L10n.t("Not fetched yet"))
                        .foregroundStyle(.secondary)
                }
                LabeledContent(L10n.t("Per-token prices")) {
                    Text(L10n.t("\(prices.prices.count) models")).foregroundStyle(.secondary)
                }
                LabeledContent(L10n.t("Plan catalog")) {
                    HStack(spacing: 8) {
                        Text(L10n.t("\(catalog.plans.count) plans")).foregroundStyle(.secondary)
                        Button(L10n.t("Open catalog")) { NSWorkspace.shared.activateFileViewerSelecting([PlanCatalog.fileURL]) }
                            .disabled(CostRuntime.isInert)
                    }
                }
                Button(L10n.t("Refresh now")) { prices.refreshIfDue(force: true); catalog.refreshIfDue(force: true) }
                    .disabled(CostRuntime.isInert)
                Text(L10n.t("Refreshed once a day, no account needed: exchange rate from open.er-api.com, per-token prices from OpenRouter. Plans live in plans.json, editable."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Section {
                Button(L10n.t("Open Activity…")) { Costs.showActivity() }
                Text(L10n.t("Day, week and month per login and project: sessions with their estimated cost in INR."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .formStyle(.grouped)
    }
}

/// One login's billing: how it is paid, what it costs, and the plan detected.
private struct CostAccountRows: View {
    let account: CostAccount
    @ObservedObject private var accounts = CostAccountStore.shared
    @ObservedObject private var prices = PriceTable.shared
    @ObservedObject private var catalog = PlanCatalog.shared

    private var listPrice: Double? {
        account.planTier.flatMap { catalog.monthlyINR(for: $0, rate: prices.rate) }
    }

    var body: some View {
        Picker(L10n.t("Billing"), selection: Binding(get: { account.billing }, set: { accounts.setBilling(account.id, $0) })) {
            ForEach(CostAccount.Billing.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented)

        if account.billing == .subscription {
            LabeledContent(L10n.t("You pay per month (INR)")) {
                TextField("", value: Binding(get: { account.monthlyPaidINR },
                                             set: { accounts.setMonthlyPaidINR(account.id, $0) }),
                          format: .number,
                          prompt: Text(listPrice.map(MoneyFormat.inr) ?? "-"))
                    .textFieldStyle(.roundedBorder)
                    .multilineTextAlignment(.trailing)
                    .frame(width: 120)
            }
            LabeledContent(L10n.t("Plan")) {
                Text(planDetail).foregroundStyle(.secondary)
            }
        }
    }

    private var planDetail: String {
        guard let tier = account.planTier else { return L10n.t("Plan not detected yet") }
        let name = catalog.name(for: tier)
        if let listPrice {
            return L10n.t("\(name) · \(MoneyFormat.inr(listPrice))/month (list price)")
        }
        return L10n.t("\(name) · price unknown, set it here")
    }
}
